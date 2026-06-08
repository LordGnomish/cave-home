// SPDX-License-Identifier: Apache-2.0
//! Top-level scheduler — wires source/sink + cache + queue + framework.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/scheduler.go

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::cache::SchedulerCache;
use crate::config::SchedulerConfig;
use crate::framework::{ActionType, ClusterEvent, Code, CycleState, Gvk, PluginRegistry};
use crate::queue::{PriorityQueue, QueuedPodInfo, SchedulingQueue};
use crate::schedule_one::{schedule_one_limited, ScheduleResult};
use crate::source_sink::{NodeEvent, PodEvent, Result, SchedulerSink, SchedulerSource};

/// Upstream: `pkg/scheduler/scheduler.go::Scheduler`.
pub struct Scheduler {
    pub source: Arc<dyn SchedulerSource>,
    pub sink: Arc<dyn SchedulerSink>,
    pub cache: SchedulerCache,
    pub queue: PriorityQueue,
    pub registry: PluginRegistry,
    /// Profile name — `default-scheduler` in Phase 2.
    pub profile_name: String,
    /// Scheduler configuration (percentage-of-nodes-to-score, profile).
    pub config: SchedulerConfig,
    /// Pods currently parked in the [`Code::Wait`] Permit disposition, keyed by
    /// pod uid. Upstream: `framework.waitingPodsMap`. An external caller (e.g. a
    /// sibling plugin reacting to a cluster event) resolves a waiting pod via
    /// [`get_waiting_pod`](Self::get_waiting_pod).
    waiting_pods: Arc<parking_lot::Mutex<std::collections::HashMap<String, crate::framework::WaitingPod>>>,
}

/// Outcome of a single `run_once` cycle (the pod that was processed +
/// the schedule result, if any).
#[derive(Debug)]
pub struct CycleOutcome {
    pub pod_full_name: String,
    pub result: Option<ScheduleResult>,
}

impl Scheduler {
    /// Construct a scheduler with the default profile.
    pub fn new(
        source: Arc<dyn SchedulerSource>,
        sink: Arc<dyn SchedulerSink>,
    ) -> Self {
        let config = SchedulerConfig::default();
        Self {
            source,
            sink,
            cache: SchedulerCache::new(),
            queue: PriorityQueue::new(),
            registry: crate::plugins::default_registry(),
            profile_name: config.profile_name.clone(),
            config,
            waiting_pods: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Look up the [`WaitingPod`](crate::framework::WaitingPod) gate for a pod
    /// currently held in the [`Code::Wait`] Permit disposition, if any. Callers
    /// use it to [`allow`](crate::framework::WaitingPod::allow) /
    /// [`reject`](crate::framework::WaitingPod::reject) the pod, releasing it
    /// from the binding cycle. Upstream: `framework.GetWaitingPod`.
    #[must_use]
    pub fn get_waiting_pod(&self, pod_uid: &str) -> Option<crate::framework::WaitingPod> {
        self.waiting_pods.lock().get(pod_uid).cloned()
    }

    /// Replace the scheduler configuration.
    #[must_use]
    pub fn with_config(mut self, config: SchedulerConfig) -> Self {
        self.profile_name = config.profile_name.clone();
        self.config = config;
        self
    }

    /// Replace the registry — useful for tests that exercise a subset.
    #[must_use]
    pub fn with_registry(mut self, reg: PluginRegistry) -> Self {
        self.registry = reg;
        self
    }

    /// Synchronise cache + queue from the source (idempotent).
    /// Upstream: the informer event handlers call into the cache;
    /// here a single `sync` poll suffices for the in-memory binding.
    pub async fn sync(&self) -> Result<()> {
        for n in self.source.list_nodes().await? {
            self.cache.add_node(n);
        }
        for p in self.source.list_pending_pods().await? {
            // Skip pods we already have queued or assumed.
            if self.cache.is_assumed(&p.metadata.uid) {
                continue;
            }
            self.admit_pod(p, 0);
        }
        Ok(())
    }

    /// Upstream: `pkg/scheduler/eventhandlers.go::addPodToSchedulingQueue` →
    /// `PriorityQueue.Add`, which first runs the profile's PreEnqueue plugins.
    ///
    /// Every registered [`PreEnqueuePlugin`](crate::framework::PreEnqueuePlugin)
    /// must return success for the pod to enter the active queue. If any gates
    /// it out, the pod is parked in the unschedulable set (it is *not* dropped)
    /// so a later cluster event re-evaluates it — exactly the
    /// `unschedulablePods` path a failed scheduling attempt takes.
    fn admit_pod(&self, pod: crate::types::Pod, now_ms: u64) {
        for plugin in self.registry.pre_enqueues() {
            let status = plugin.pre_enqueue(&pod);
            if !status.is_success() {
                let info = QueuedPodInfo::new(pod);
                let cycle = self.queue.scheduling_cycle();
                self.queue.add_unschedulable_if_not_present(info, cycle, now_ms);
                return;
            }
        }
        self.queue.add(pod);
    }

    /// Drain a single pod from the queue and attempt to schedule it.
    /// Upstream: `scheduler.scheduleOne`.
    pub async fn run_once(&self) -> Result<Option<CycleOutcome>> {
        let Some(info) = self.queue.pop() else {
            return Ok(None);
        };
        let pod = info.pod.clone();
        let full = pod.full_name();
        let limit = self.config.num_feasible_nodes_to_find(self.cache.node_count());
        let result = schedule_one_limited(&pod, &self.cache, &self.registry, limit);

        if let Some(host) = result.suggested_host.clone() {
            // Drive the full binding cycle (Reserve → Permit → PreBind → Bind →
            // PostBind, with Unreserve rollback). On failure we re-queue with
            // backoff via the unschedulable set.
            match self.run_binding_cycle(&pod, &host).await {
                Ok(()) => {
                    let _ = self
                        .sink
                        .record_event(
                            &pod,
                            "Scheduled",
                            &format!(
                                "Successfully assigned {full} to {host} (profile={})",
                                self.profile_name
                            ),
                        )
                        .await;
                }
                Err(reason) => {
                    let _ = self
                        .sink
                        .record_event(&pod, "FailedScheduling", &reason)
                        .await;
                    self.queue.add_unschedulable(info);
                }
            }
        } else if let Some(nominee) = &result.nominated_node {
            // Preemption nominated a node; the next cycle (after victim
            // eviction by the API server) will re-evaluate. Re-queue.
            let _ = self
                .sink
                .record_event(
                    &pod,
                    "Preempted",
                    &format!("preempted lower-priority pods on {nominee}"),
                )
                .await;
            self.queue.add_unschedulable(info);
        } else {
            let reasons: Vec<String> = result
                .filter_failures
                .values()
                .map(crate::framework::Status::message)
                .collect();
            let _ = self
                .sink
                .record_event(
                    &pod,
                    "FailedScheduling",
                    &format!("no nodes available; reasons: {}", reasons.join(", ")),
                )
                .await;
            self.queue.add_unschedulable(info);
        }

        Ok(Some(CycleOutcome {
            pod_full_name: full,
            result: Some(result),
        }))
    }

    /// Run cycles until the active queue is empty.
    pub async fn drain_active(&self) -> Result<Vec<CycleOutcome>> {
        let mut out = Vec::new();
        while let Some(c) = self.run_once().await? {
            out.push(c);
        }
        Ok(out)
    }

    /// Upstream: `pkg/scheduler/scheduler.go::Scheduler.Run`.
    ///
    /// The real event-driven loop. Three concurrent informers feed the queue
    /// and cache from the source's watch streams (pods, nodes) and a periodic
    /// flush promotes backed-off / leftover pods; the main loop blocks on
    /// `queue.pop_wait()` and schedules+binds each pod as it becomes ready.
    /// Runs until `cancel` is notified.
    pub async fn run(self: Arc<Self>, cancel: Arc<Notify>) {
        let start = Instant::now();
        let now_ms = move || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let pod_task = tokio::spawn(Self::watch_pods_loop(self.clone(), now_ms));
        let node_task = tokio::spawn(Self::watch_nodes_loop(self.clone(), now_ms));
        let flush_task = tokio::spawn(Self::flush_loop(self.queue.clone(), now_ms));

        // Upstream `wait.UntilWithContext(ctx, sched.scheduleOne, 0)`.
        loop {
            tokio::select! {
                biased;
                () = cancel.notified() => break,
                maybe = self.queue.pop_wait() => match maybe {
                    Some(info) => self.schedule_and_bind(info, now_ms()).await,
                    None => break,
                },
            }
        }

        self.queue.close();
        pod_task.abort();
        node_task.abort();
        flush_task.abort();
    }

    /// Informer: fold pod watch events into the queue/cache.
    /// Upstream: `pkg/scheduler/eventhandlers.go::addPodToSchedulingQueue` etc.
    async fn watch_pods_loop(self: Arc<Self>, now_ms: impl Fn() -> u64) {
        let Ok(mut stream) = self.source.watch_pods().await else {
            return;
        };
        while let Some(ev) = stream.recv().await {
            match ev {
                PodEvent::Add(p) | PodEvent::Update { new: p, .. } => {
                    if p.spec.node_name.is_empty() {
                        // Still pending → run the PreEnqueue gate, then (re)enqueue.
                        self.admit_pod(p, now_ms());
                    } else {
                        // Already bound elsewhere → reflect in the cache.
                        let _ = self.cache.add_pod(p);
                    }
                }
                PodEvent::Delete(p) => {
                    let _ = self.cache.remove_pod(&p);
                    // Freed capacity may unblock waiters.
                    let ev = ClusterEvent::new(Gvk::Pod, ActionType::DELETE);
                    self.queue.move_all_to_active_or_backoff_queue(&ev, now_ms());
                }
            }
        }
    }

    /// Informer: fold node watch events into the cache and wake pods waiting on
    /// node changes. Upstream: `pkg/scheduler/eventhandlers.go::addNodeToCache`.
    async fn watch_nodes_loop(self: Arc<Self>, now_ms: impl Fn() -> u64) {
        let Ok(mut stream) = self.source.watch_nodes().await else {
            return;
        };
        while let Some(ev) = stream.recv().await {
            match ev {
                NodeEvent::Add(n) => {
                    self.cache.add_node(n);
                    let ev = ClusterEvent::new(Gvk::Node, ActionType::ADD);
                    self.queue.move_all_to_active_or_backoff_queue(&ev, now_ms());
                }
                NodeEvent::Update { new, .. } => {
                    self.cache.update_node(new);
                    let ev = ClusterEvent::new(
                        Gvk::Node,
                        ActionType::UPDATE_NODE_ALLOCATABLE
                            | ActionType::UPDATE_NODE_TAINT
                            | ActionType::UPDATE_NODE_LABEL,
                    );
                    self.queue.move_all_to_active_or_backoff_queue(&ev, now_ms());
                }
                NodeEvent::Delete(n) => {
                    self.cache.remove_node(&n.metadata.name);
                }
            }
        }
    }

    /// Periodic flush: promote backed-off pods whose timer elapsed and rescue
    /// pods stranded in the unschedulable set past the leftover threshold.
    /// Upstream: the `flushBackoffQCompleted` / `flushUnschedulablePodsLeftover`
    /// timers in `pkg/scheduler/backend/queue`.
    async fn flush_loop(queue: PriorityQueue, now_ms: impl Fn() -> u64) {
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        loop {
            tick.tick().await;
            let now = now_ms();
            queue.flush_backoff(now);
            queue.flush_unschedulable_pods_leftover(now);
        }
    }

    /// Schedule a single popped pod and drive its binding, re-queueing it on
    /// any failure. Shared bind path for the event loop.
    async fn schedule_and_bind(&self, info: QueuedPodInfo, now_ms: u64) {
        // The scheduling cycle this pod was popped in (set by `pop_wait`).
        let pod_cycle = self.queue.scheduling_cycle();
        let pod = info.pod.clone();
        let full = pod.full_name();
        let limit = self.config.num_feasible_nodes_to_find(self.cache.node_count());
        let result = schedule_one_limited(&pod, &self.cache, &self.registry, limit);

        if let Some(host) = result.suggested_host.clone() {
            match self.run_binding_cycle(&pod, &host).await {
                Ok(()) => {
                    let _ = self
                        .sink
                        .record_event(
                            &pod,
                            "Scheduled",
                            &format!(
                                "Successfully assigned {full} to {host} (profile={})",
                                self.profile_name
                            ),
                        )
                        .await;
                }
                Err(reason) => {
                    let _ = self
                        .sink
                        .record_event(&pod, "FailedScheduling", &reason)
                        .await;
                    self.queue
                        .add_unschedulable_if_not_present(info, pod_cycle, now_ms);
                }
            }
        } else if let Some(nominee) = result.nominated_node.clone() {
            let _ = self
                .sink
                .record_event(
                    &pod,
                    "Preempted",
                    &format!("preempted lower-priority pods on {nominee}"),
                )
                .await;
            self.queue
                .add_unschedulable_if_not_present(info, pod_cycle, now_ms);
        } else {
            let reasons: Vec<String> = result
                .filter_failures
                .values()
                .map(crate::framework::Status::message)
                .collect();
            let _ = self
                .sink
                .record_event(
                    &pod,
                    "FailedScheduling",
                    &format!("no nodes available; reasons: {}", reasons.join(", ")),
                )
                .await;
            self.queue
                .add_unschedulable_if_not_present(info, pod_cycle, now_ms);
        }
    }

    /// Upstream: the binding cycle — `Reserve -> Permit -> PreBind -> Bind`,
    /// with `Unreserve` rollback if any stage rejects. The built-in
    /// node-resource reservation is the cache `assume_pod`; the bind itself is
    /// the `DefaultBinder` (the sink). On any failure the assumption and every
    /// Reserve plugin that ran are unwound, and an `Err(reason)` is returned so
    /// the caller re-queues the pod.
    async fn run_binding_cycle(
        &self,
        pod: &crate::types::Pod,
        host: &str,
    ) -> std::result::Result<(), String> {
        // Built-in Reserve: tentatively place the pod (claims its resources).
        self.cache
            .assume_pod(pod.clone(), host)
            .map_err(|e| format!("assume failed: {e}"))?;

        let mut state = CycleState::new();

        // ---------- Reserve ----------
        let mut reserved = 0_usize;
        for plugin in self.registry.reserves() {
            reserved += 1;
            let status = plugin.reserve(&mut state, pod, host);
            if !status.is_success() {
                self.unwind(&mut state, pod, host, reserved);
                return Err(format!(
                    "reserve {} failed: {}",
                    plugin.name(),
                    status.message()
                ));
            }
        }

        // ---------- Permit ----------
        // Each Permit plugin approves (Success), rejects (other non-success), or
        // asks to hold the pod (Wait). Wait dispositions accumulate: the pod is
        // parked on the shortest requested timeout until allowed/rejected
        // externally, or the timeout fires (treated as a rejection). Upstream:
        // `RunPermitPlugins` + `WaitOnPermit`.
        let mut wait_timeout: Option<Duration> = None;
        for plugin in self.registry.permits() {
            let status = plugin.permit(&mut state, pod, host);
            if status.is_wait() {
                let t = plugin.permit_timeout();
                wait_timeout = Some(wait_timeout.map_or(t, |cur| cur.min(t)));
                continue;
            }
            if !status.is_success() {
                self.unwind(&mut state, pod, host, reserved);
                return Err(format!(
                    "permit {} denied: {}",
                    plugin.name(),
                    status.message()
                ));
            }
        }
        if let Some(timeout) = wait_timeout {
            if let Err(reason) = self.wait_on_permit(pod, timeout).await {
                self.unwind(&mut state, pod, host, reserved);
                return Err(reason);
            }
        }

        // ---------- PreBind ----------
        for plugin in self.registry.pre_binds() {
            let status = plugin.pre_bind(&mut state, pod, host);
            if !status.is_success() {
                self.unwind(&mut state, pod, host, reserved);
                return Err(format!(
                    "prebind {} failed: {}",
                    plugin.name(),
                    status.message()
                ));
            }
        }

        // ---------- Bind ----------
        // Bind plugins run in registration order; the first to return a non-Skip
        // status owns the bind. If every plugin abstains (or none is registered)
        // the built-in DefaultBinder — the sink's Binding POST — performs it.
        let mut bound = false;
        for plugin in self.registry.binds() {
            let status = plugin.bind(&mut state, pod, host).await;
            if status.code == Code::Skip {
                continue;
            }
            if !status.is_success() {
                self.unwind(&mut state, pod, host, reserved);
                return Err(format!("bind {} failed: {}", plugin.name(), status.message()));
            }
            bound = true;
            break;
        }
        if !bound {
            if let Err(e) = self.sink.bind(pod, host).await {
                self.unwind(&mut state, pod, host, reserved);
                return Err(format!("bind failed: {e}"));
            }
        }

        // ---------- PostBind ----------
        // Best-effort, success-path-only: the pod is bound, so these callbacks
        // cannot fail the cycle.
        for plugin in self.registry.post_binds() {
            plugin.post_bind(&mut state, pod, host);
        }
        Ok(())
    }

    /// Upstream: `framework.WaitOnPermit`. Park the pod on a [`WaitingPod`] gate
    /// for at most `timeout`, blocking the binding cycle until it is
    /// [`allow`](crate::framework::WaitingPod::allow)ed (proceed to bind),
    /// [`reject`](crate::framework::WaitingPod::reject)ed, or the timeout fires
    /// (treated as a rejection). The gate is registered under the pod uid so an
    /// external caller can resolve it via [`get_waiting_pod`](Self::get_waiting_pod),
    /// and is always deregistered before returning.
    async fn wait_on_permit(
        &self,
        pod: &crate::types::Pod,
        timeout: Duration,
    ) -> std::result::Result<(), String> {
        use crate::framework::PermitDecision;

        let uid = pod.metadata.uid.clone();
        let (wp, rx) = crate::framework::WaitingPod::new(uid.clone());
        self.waiting_pods.lock().insert(uid.clone(), wp);

        let outcome = tokio::time::timeout(timeout, rx).await;
        // Resolve and deregister regardless of outcome.
        self.waiting_pods.lock().remove(&uid);

        match outcome {
            Ok(Ok(PermitDecision::Allow)) => Ok(()),
            Ok(Ok(PermitDecision::Reject(reason))) => {
                Err(format!("permit rejected while waiting: {reason}"))
            }
            // Sender dropped without deciding — treat as a rejection so the pod
            // is unwound and re-queued rather than stranded.
            Ok(Err(_)) => Err("permit wait aborted".to_string()),
            Err(_) => Err(format!(
                "permit wait timed out after {}ms",
                timeout.as_millis()
            )),
        }
    }

    /// Roll back a partial binding cycle: Unreserve the Reserve plugins that
    /// ran (reverse order) and release the cache assumption.
    fn unwind(&self, state: &mut CycleState, pod: &crate::types::Pod, host: &str, reserved: usize) {
        for plugin in self.registry.reserves().iter().take(reserved).rev() {
            plugin.unreserve(state, pod, host);
        }
        self.cache.forget_pod(&pod.metadata.uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_sink::{InMemorySink, InMemorySource};
    use crate::types::{Container, Node, ObjectMeta, Pod, Quantity, ResourceName};

    fn node(name: &str, cpu: i64, mem: i64) -> Node {
        let mut n = Node::default();
        n.metadata.name = name.into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(mem));
        n
    }

    fn pod(name: &str, cpu: i64, mem: i64) -> Pod {
        let mut p = Pod::default();
        p.metadata = ObjectMeta {
            namespace: "default".into(),
            name: name.into(),
            uid: name.into(),
            ..Default::default()
        };
        let mut c = Container::default();
        c.resources
            .requests
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
        c.resources
            .requests
            .insert(ResourceName::Memory, Quantity::bytes(mem));
        p.spec.containers.push(c);
        p
    }

    #[tokio::test]
    async fn end_to_end_schedule_and_bind() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        src.add_node(node("n1", 1000, 1024));
        src.add_pod(pod("alpha", 100, 256));

        let sched = Scheduler::new(src.clone(), sink.clone());
        sched.sync().await.unwrap();
        let cycle = sched.run_once().await.unwrap().unwrap();
        assert_eq!(cycle.pod_full_name, "default/alpha");
        assert_eq!(sink.binds(), vec![("default/alpha".into(), "n1".into())]);
    }

    #[tokio::test]
    async fn unschedulable_pod_is_requeued() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        src.add_node(node("tiny", 100, 256));
        src.add_pod(pod("huge", 5_000, 1));

        let sched = Scheduler::new(src.clone(), sink.clone());
        sched.sync().await.unwrap();
        let _ = sched.run_once().await.unwrap();
        assert!(sink.binds().is_empty());
        assert!(sink
            .events()
            .iter()
            .any(|(_, r, _)| r == "FailedScheduling"));
        // Queue length includes the backoff slot.
        assert_eq!(sched.queue.len(), 1);
    }

    #[tokio::test]
    async fn higher_priority_pod_pops_first() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        src.add_node(node("n1", 10_000, 100_000));
        let mut low = pod("low", 100, 100);
        low.spec.priority = 0;
        let mut high = pod("high", 100, 100);
        high.spec.priority = 100;
        src.add_pod(low);
        src.add_pod(high);

        let sched = Scheduler::new(src.clone(), sink.clone());
        sched.sync().await.unwrap();
        let outcomes = sched.drain_active().await.unwrap();
        assert_eq!(outcomes[0].pod_full_name, "default/high");
        assert_eq!(outcomes[1].pod_full_name, "default/low");
    }

    #[tokio::test]
    async fn binds_only_to_node_passing_taint() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());

        let mut tainted = node("tainted", 4000, 4096);
        tainted.spec.taints.push(crate::types::Taint {
            key: "k".into(),
            value: "v".into(),
            effect: crate::types::TaintEffect::NoSchedule,
        });
        let clean = node("clean", 4000, 4096);
        src.add_node(tainted);
        src.add_node(clean);
        src.add_pod(pod("a", 100, 100));

        let sched = Scheduler::new(src.clone(), sink.clone());
        sched.sync().await.unwrap();
        sched.run_once().await.unwrap();
        assert_eq!(sink.binds(), vec![("default/a".into(), "clean".into())]);
    }

    // ---------- Binding cycle (Reserve / Permit / PreBind) ----------

    use crate::framework::{CycleState, PermitPlugin, ReservePlugin, Status};
    use crate::queue::QueuedPodInfo;

    #[derive(Default)]
    struct Calls {
        log: std::sync::Mutex<Vec<String>>,
    }
    impl Calls {
        fn push(&self, s: String) {
            self.log.lock().unwrap().push(s);
        }
        fn snapshot(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    struct RecordingReserve(Arc<Calls>);
    impl ReservePlugin for RecordingReserve {
        fn name(&self) -> &'static str {
            "RecordingReserve"
        }
        fn reserve(&self, _: &mut CycleState, pod: &Pod, node_name: &str) -> Status {
            self.0
                .push(format!("reserve:{}:{node_name}", pod.metadata.name));
            Status::success()
        }
        fn unreserve(&self, _: &mut CycleState, pod: &Pod, node_name: &str) {
            self.0
                .push(format!("unreserve:{}:{node_name}", pod.metadata.name));
        }
    }

    struct DenyPermit;
    impl PermitPlugin for DenyPermit {
        fn name(&self) -> &'static str {
            "DenyPermit"
        }
        fn permit(&self, _: &mut CycleState, _: &Pod, _: &str) -> Status {
            Status::unschedulable("DenyPermit", "permit denied")
        }
    }

    #[tokio::test]
    async fn binding_cycle_reserves_then_binds_on_success() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_reserve(Arc::new(RecordingReserve(calls.clone())))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("alpha", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        assert_eq!(sink.binds(), vec![("default/alpha".into(), "n1".into())]);
        let log = calls.snapshot();
        assert!(log.contains(&"reserve:alpha:n1".to_string()));
        assert!(!log.iter().any(|s| s.starts_with("unreserve")));
    }

    use crate::framework::{BindPlugin, Code, PostBindPlugin};

    struct RecordingBind(Arc<Calls>);
    #[async_trait::async_trait]
    impl BindPlugin for RecordingBind {
        fn name(&self) -> &'static str {
            "RecordingBind"
        }
        async fn bind(&self, _: &mut CycleState, pod: &Pod, node_name: &str) -> Status {
            self.0
                .push(format!("bind:{}:{node_name}", pod.metadata.name));
            Status::success()
        }
    }

    struct SkipBind(Arc<Calls>);
    #[async_trait::async_trait]
    impl BindPlugin for SkipBind {
        fn name(&self) -> &'static str {
            "SkipBind"
        }
        async fn bind(&self, _: &mut CycleState, pod: &Pod, _: &str) -> Status {
            self.0.push(format!("skip:{}", pod.metadata.name));
            Status::skip(self.name())
        }
    }

    #[tokio::test]
    async fn custom_bind_plugin_intercepts_default_binder() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_bind(Arc::new(RecordingBind(calls.clone())))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("gamma", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        // The custom bind plugin owned the bind; the DefaultBinder (sink) was
        // never consulted.
        assert!(calls.snapshot().contains(&"bind:gamma:n1".to_string()));
        assert!(sink.binds().is_empty());
        // Success still surfaces a Scheduled event.
        assert!(sink.events().iter().any(|(_, r, _)| r == "Scheduled"));
    }

    #[tokio::test]
    async fn bind_plugin_skip_falls_through_to_default_binder() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_bind(Arc::new(SkipBind(calls.clone())))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("delta", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        // The skip plugin was consulted but abstained, so the DefaultBinder bound.
        assert!(calls.snapshot().contains(&"skip:delta".to_string()));
        assert_eq!(sink.binds(), vec![("default/delta".into(), "n1".into())]);
    }

    struct RecordingPostBind(Arc<Calls>);
    impl PostBindPlugin for RecordingPostBind {
        fn name(&self) -> &'static str {
            "RecordingPostBind"
        }
        fn post_bind(&self, _: &mut CycleState, pod: &Pod, node_name: &str) {
            self.0
                .push(format!("postbind:{}:{node_name}", pod.metadata.name));
        }
    }

    #[tokio::test]
    async fn post_bind_runs_after_successful_bind() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_post_bind(Arc::new(RecordingPostBind(calls.clone())))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("epsilon", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        assert_eq!(sink.binds(), vec![("default/epsilon".into(), "n1".into())]);
        let log = calls.snapshot();
        assert!(log.contains(&"postbind:epsilon:n1".to_string()));
    }

    #[tokio::test]
    async fn post_bind_skipped_when_binding_cycle_fails() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        // Permit denies before the bind, so the pod is never bound and PostBind
        // must not run.
        let reg = PluginRegistry::builder()
            .with_permit(Arc::new(DenyPermit))
            .with_post_bind(Arc::new(RecordingPostBind(calls.clone())))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("zeta", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        assert!(sink.binds().is_empty());
        assert!(!calls.snapshot().iter().any(|s| s.starts_with("postbind")));
    }

    // ---------- PreEnqueue (queue-admission gate) ----------

    use crate::framework::PreEnqueuePlugin;

    /// Gates out any pod whose label `gate=open` is absent.
    struct LabelGate;
    impl PreEnqueuePlugin for LabelGate {
        fn name(&self) -> &'static str {
            "LabelGate"
        }
        fn pre_enqueue(&self, pod: &Pod) -> Status {
            if pod.metadata.labels.get("gate").map(String::as_str) == Some("open") {
                Status::success()
            } else {
                Status::unschedulable("LabelGate", "gate not open")
            }
        }
    }

    #[tokio::test]
    async fn pre_enqueue_gate_holds_pod_out_of_active_queue() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let reg = PluginRegistry::builder()
            .with_pre_enqueue(Arc::new(LabelGate))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        // Gated pod (no label) is parked, not scheduled.
        src.add_pod(pod("gated", 100, 256));
        sched.sync().await.unwrap();
        assert!(sched.run_once().await.unwrap().is_none());
        assert!(sink.binds().is_empty());
        // It lives in the unschedulable set, not the active queue.
        assert_eq!(sched.queue.unschedulable_count(), 1);
    }

    #[tokio::test]
    async fn pre_enqueue_admits_pod_when_gate_open() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let reg = PluginRegistry::builder()
            .with_pre_enqueue(Arc::new(LabelGate))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let mut p = pod("open", 100, 256);
        p.metadata.labels.insert("gate".into(), "open".into());
        src.add_pod(p);
        sched.sync().await.unwrap();
        sched.run_once().await.unwrap();
        assert_eq!(sink.binds(), vec![("default/open".into(), "n1".into())]);
    }

    #[tokio::test]
    async fn permit_denial_unreserves_and_requeues() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_reserve(Arc::new(RecordingReserve(calls.clone())))
            .with_permit(Arc::new(DenyPermit))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("beta", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        // Permit denied -> no bind happened.
        assert!(sink.binds().is_empty());
        // Reserve must have been rolled back via Unreserve.
        let log = calls.snapshot();
        assert!(log.contains(&"reserve:beta:n1".to_string()));
        assert!(log.contains(&"unreserve:beta:n1".to_string()));
        // The assumed placement is released from the cache.
        assert!(!sched.cache.is_assumed("beta"));
        // The pod is re-queued for a later attempt.
        assert!(sched.queue.unschedulable_count() + sched.queue.backoff_count() >= 1);
    }

    // ---------- Permit Wait disposition ----------

    /// Holds every pod with a generous timeout; only an external allow/reject
    /// (or that timeout) releases it.
    struct HoldPermit(Duration);
    impl PermitPlugin for HoldPermit {
        fn name(&self) -> &'static str {
            "HoldPermit"
        }
        fn permit(&self, _: &mut CycleState, _: &Pod, _: &str) -> Status {
            Status::wait(self.name())
        }
        fn permit_timeout(&self) -> Duration {
            self.0
        }
    }

    #[tokio::test]
    async fn permit_wait_then_allow_proceeds_to_bind() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_reserve(Arc::new(RecordingReserve(calls.clone())))
            .with_permit(Arc::new(HoldPermit(Duration::from_secs(30))))
            .build();
        let sched = Arc::new(Scheduler::new(src.clone(), sink.clone()).with_registry(reg));
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("waiter", 100, 256));
        let s2 = sched.clone();
        let handle = tokio::spawn(async move { s2.schedule_and_bind(info, 0).await });

        // Let the binding cycle reach the Permit wait and register the gate.
        let wp = loop {
            if let Some(wp) = sched.get_waiting_pod("waiter") {
                break wp;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(wp.pod_uid(), "waiter");
        assert!(wp.allow());

        handle.await.unwrap();
        assert_eq!(sink.binds(), vec![("default/waiter".into(), "n1".into())]);
        // Reserve held, never rolled back.
        assert!(!calls.snapshot().iter().any(|s| s.starts_with("unreserve")));
        // The gate is deregistered once resolved.
        assert!(sched.get_waiting_pod("waiter").is_none());
    }

    #[tokio::test]
    async fn permit_wait_then_reject_unreserves_and_requeues() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        let reg = PluginRegistry::builder()
            .with_reserve(Arc::new(RecordingReserve(calls.clone())))
            .with_permit(Arc::new(HoldPermit(Duration::from_secs(30))))
            .build();
        let sched = Arc::new(Scheduler::new(src.clone(), sink.clone()).with_registry(reg));
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("rejectee", 100, 256));
        let s2 = sched.clone();
        let handle = tokio::spawn(async move { s2.schedule_and_bind(info, 0).await });

        let wp = loop {
            if let Some(wp) = sched.get_waiting_pod("rejectee") {
                break wp;
            }
            tokio::task::yield_now().await;
        };
        assert!(wp.reject("no longer wanted"));

        handle.await.unwrap();
        assert!(sink.binds().is_empty());
        let log = calls.snapshot();
        assert!(log.contains(&"reserve:rejectee:n1".to_string()));
        assert!(log.contains(&"unreserve:rejectee:n1".to_string()));
        assert!(!sched.cache.is_assumed("rejectee"));
        assert!(sched.queue.unschedulable_count() + sched.queue.backoff_count() >= 1);
    }

    #[tokio::test]
    async fn permit_wait_timeout_unreserves_and_requeues() {
        let src = Arc::new(InMemorySource::new());
        let sink = Arc::new(InMemorySink::new());
        let calls = Arc::new(Calls::default());
        // Tiny timeout, never allowed -> the timeout fires and rejects.
        let reg = PluginRegistry::builder()
            .with_reserve(Arc::new(RecordingReserve(calls.clone())))
            .with_permit(Arc::new(HoldPermit(Duration::from_millis(20))))
            .build();
        let sched = Scheduler::new(src.clone(), sink.clone()).with_registry(reg);
        sched.cache.add_node(node("n1", 1000, 1024));

        let info = QueuedPodInfo::new(pod("timeouter", 100, 256));
        sched.schedule_and_bind(info, 0).await;

        assert!(sink.binds().is_empty());
        let log = calls.snapshot();
        assert!(log.contains(&"reserve:timeouter:n1".to_string()));
        assert!(log.contains(&"unreserve:timeouter:n1".to_string()));
        assert!(!sched.cache.is_assumed("timeouter"));
        assert!(sched.queue.unschedulable_count() + sched.queue.backoff_count() >= 1);
    }
}
