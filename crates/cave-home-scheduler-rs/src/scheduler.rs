// SPDX-License-Identifier: Apache-2.0
//! Top-level scheduler — wires source/sink + cache + queue + framework.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/scheduler.go

use std::sync::Arc;

use crate::cache::SchedulerCache;
use crate::framework::PluginRegistry;
use crate::queue::{PriorityQueue, SchedulingQueue};
use crate::schedule_one::{schedule_one, ScheduleResult};
use crate::source_sink::{Result, SchedulerSink, SchedulerSource};

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
