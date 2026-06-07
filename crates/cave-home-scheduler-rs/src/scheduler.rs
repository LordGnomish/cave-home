// SPDX-License-Identifier: Apache-2.0
//! Top-level scheduler — wires source/sink + cache + queue + framework.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/scheduler.go

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::cache::SchedulerCache;
use crate::framework::{ActionType, ClusterEvent, Gvk, PluginRegistry};
use crate::queue::{PriorityQueue, QueuedPodInfo, SchedulingQueue};
use crate::schedule_one::{schedule_one, ScheduleResult};
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
        Self {
            source,
            sink,
            cache: SchedulerCache::new(),
            queue: PriorityQueue::new(),
            registry: crate::plugins::default_registry(),
            profile_name: "default-scheduler".into(),
        }
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
            self.queue.add(p);
        }
        Ok(())
    }

    /// Drain a single pod from the queue and attempt to schedule it.
    /// Upstream: `scheduler.scheduleOne`.
    pub async fn run_once(&self) -> Result<Option<CycleOutcome>> {
        let Some(info) = self.queue.pop() else {
            return Ok(None);
        };
        let pod = info.pod.clone();
        let full = pod.full_name();
        let result = schedule_one(&pod, &self.cache, &self.registry);

        if let Some(host) = &result.suggested_host {
            // Assume the pod into the cache and emit the bind.
            // If the cache rejects (e.g. the node disappeared), we
            // surface as Schedulable=false and re-queue with backoff.
            match self.cache.assume_pod(pod.clone(), host) {
                Ok(()) => match self.sink.bind(&pod, host).await {
                    Ok(()) => {
                        let _ = self
                            .sink
                            .record_event(
                                &pod,
                                "Scheduled",
                                &format!(
                                    "Successfully assigned {} to {host} (profile={})",
                                    full, self.profile_name
                                ),
                            )
                            .await;
                    }
                    Err(e) => {
                        self.cache.forget_pod(&pod.metadata.uid);
                        let _ = self
                            .sink
                            .record_event(
                                &pod,
                                "FailedScheduling",
                                &format!("bind failed: {e}"),
                            )
                            .await;
                        self.queue.add_unschedulable(info);
                    }
                },
                Err(e) => {
                    let _ = self
                        .sink
                        .record_event(
                            &pod,
                            "FailedScheduling",
                            &format!("assume failed: {e}"),
                        )
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
                .map(|s| s.message())
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
                        // Still pending → (re)enqueue for scheduling.
                        self.queue.add(p);
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
        let result = schedule_one(&pod, &self.cache, &self.registry);

        if let Some(host) = result.suggested_host.clone() {
            match self.cache.assume_pod(pod.clone(), &host) {
                Ok(()) => match self.sink.bind(&pod, &host).await {
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
                    Err(e) => {
                        self.cache.forget_pod(&pod.metadata.uid);
                        let _ = self
                            .sink
                            .record_event(&pod, "FailedScheduling", &format!("bind failed: {e}"))
                            .await;
                        self.queue
                            .add_unschedulable_if_not_present(info, pod_cycle, now_ms);
                    }
                },
                Err(e) => {
                    let _ = self
                        .sink
                        .record_event(&pod, "FailedScheduling", &format!("assume failed: {e}"))
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
}
