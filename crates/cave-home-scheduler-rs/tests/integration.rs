// SPDX-License-Identifier: Apache-2.0
//! End-to-end scheduler integration tests.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/scheduler_test.go (slice covered by Phase 2).

use std::sync::Arc;

use cave_home_scheduler_rs::{
    Container, InMemorySink, InMemorySource, Node, NodeSelector, NodeSelectorOperator,
    NodeSelectorRequirement, NodeSelectorTerm, ObjectMeta, Pod, PodSpec, Quantity, ResourceName,
    Scheduler, SchedulingQueue, Taint, TaintEffect, Toleration, TolerationOperator,
};

fn node(name: &str, cpu_m: i64, mem_b: i64) -> Node {
    let mut n = Node::default();
    n.metadata.name = name.into();
    n.status
        .allocatable
        .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
    n.status
        .allocatable
        .insert(ResourceName::Memory, Quantity::bytes(mem_b));
    n
}

fn pod(name: &str, cpu_m: i64, mem_b: i64) -> Pod {
    let mut p = Pod {
        metadata: ObjectMeta {
            namespace: "default".into(),
            name: name.into(),
            uid: name.into(),
            ..Default::default()
        },
        spec: PodSpec::default(),
        ..Default::default()
    };
    let mut c = Container::default();
    c.resources
        .requests
        .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
    c.resources
        .requests
        .insert(ResourceName::Memory, Quantity::bytes(mem_b));
    p.spec.containers.push(c);
    p
}

#[tokio::test]
async fn end_to_end_single_pod_single_node_binds() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());
    src.add_node(node("n1", 1000, 4096));
    src.add_pod(pod("hello", 100, 256));

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    sched.run_once().await.unwrap();

    assert_eq!(sink.binds(), vec![("default/hello".into(), "n1".into())]);
    let events = sink.events();
    assert!(events.iter().any(|(_, r, _)| r == "Scheduled"));
}

#[tokio::test]
async fn end_to_end_node_selector_routes_pod() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());

    let mut a = node("zone-a", 1000, 4096);
    a.metadata.labels.insert("zone".into(), "a".into());
    let mut b = node("zone-b", 1000, 4096);
    b.metadata.labels.insert("zone".into(), "b".into());
    src.add_node(a);
    src.add_node(b);

    let mut p = pod("zone-a-only", 100, 256);
    p.spec.node_selector.insert("zone".into(), "b".into());
    src.add_pod(p);

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    sched.run_once().await.unwrap();
    assert_eq!(
        sink.binds(),
        vec![("default/zone-a-only".into(), "zone-b".into())]
    );
}

#[tokio::test]
async fn end_to_end_required_node_affinity_routes_pod() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());

    let mut a = node("a", 1000, 4096);
    a.metadata.labels.insert("role".into(), "edge".into());
    let mut b = node("b", 1000, 4096);
    b.metadata.labels.insert("role".into(), "core".into());
    src.add_node(a);
    src.add_node(b);

    let mut p = pod("edge", 100, 256);
    p.spec.affinity = Some(cave_home_scheduler_rs::Affinity {
        node_affinity: Some(cave_home_scheduler_rs::NodeAffinity {
            required_during_scheduling: Some(NodeSelector {
                node_selector_terms: vec![NodeSelectorTerm {
                    match_expressions: vec![NodeSelectorRequirement {
                        key: "role".into(),
                        operator: Some(NodeSelectorOperator::In),
                        values: vec!["edge".into()],
                    }],
                }],
            }),
            preferred_during_scheduling: Vec::new(),
        }),
    });
    src.add_pod(p);

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    sched.run_once().await.unwrap();
    assert_eq!(sink.binds(), vec![("default/edge".into(), "a".into())]);
}

#[tokio::test]
async fn end_to_end_taint_blocks_untolerated_pod() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());

    let mut tainted = node("tainted", 1000, 4096);
    tainted.spec.taints.push(Taint {
        key: "gpu".into(),
        value: "true".into(),
        effect: TaintEffect::NoSchedule,
    });
    src.add_node(tainted);
    src.add_pod(pod("a", 100, 256));

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    sched.run_once().await.unwrap();
    assert!(sink.binds().is_empty());
}

#[tokio::test]
async fn end_to_end_tolerated_taint_allows_bind() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());

    let mut tainted = node("gpu", 1000, 4096);
    tainted.spec.taints.push(Taint {
        key: "gpu".into(),
        value: "true".into(),
        effect: TaintEffect::NoSchedule,
    });
    src.add_node(tainted);
    let mut p = pod("workload", 100, 256);
    p.spec.tolerations.push(Toleration {
        key: "gpu".into(),
        operator: TolerationOperator::Equal,
        value: "true".into(),
        effect: Some(TaintEffect::NoSchedule),
    });
    src.add_pod(p);

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    sched.run_once().await.unwrap();
    assert_eq!(sink.binds(), vec![("default/workload".into(), "gpu".into())]);
}

#[tokio::test]
async fn end_to_end_preemption_nominates_node_when_only_lower_priority_pods_block() {
    // A node already saturated by a low-priority pod cannot fit a new
    // high-priority pod on the Filter pass; PostFilter (DefaultPreemption)
    // must nominate the node by evicting the lower-priority occupant, and the
    // scheduler must surface that as a "Preempted" event (no bind yet — the
    // eviction is driven by the API server in a later cycle).
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());
    src.add_node(node("n1", 1000, 4096));

    // Occupant pod is already bound to n1 and consumes almost all CPU.
    let mut occupant = pod("occupant", 800, 256);
    occupant.spec.priority = 0;
    occupant.spec.node_name = "n1".into();
    src.add_pod(occupant);

    // Incoming high-priority pod needs more CPU than is free.
    let mut important = pod("important", 500, 256);
    important.spec.priority = 1000;
    src.add_pod(important);

    let sched = Scheduler::new(src.clone(), sink.clone());
    // Fold the already-bound occupant into the cache so NodeInfo reflects its load.
    sched.cache.add_node(node("n1", 1000, 4096));
    let mut bound = pod("occupant", 800, 256);
    bound.spec.node_name = "n1".into();
    sched.cache.add_pod(bound).unwrap();

    sched.sync().await.unwrap();
    sched.run_once().await.unwrap();

    // No bind for the important pod yet — it was nominated via preemption.
    assert!(
        sink.binds()
            .iter()
            .all(|(name, _)| name != "default/important"),
        "preempting pod must not bind in the same cycle"
    );
    assert!(
        sink.events()
            .iter()
            .any(|(p, r, _)| p == "default/important" && r == "Preempted"),
        "expected a Preempted event, got {:?}",
        sink.events()
    );
}

#[tokio::test]
async fn end_to_end_backoff_flush_requeues_unschedulable_pod() {
    // An unschedulable pod is moved to the backoff sub-queue; flushing the
    // backoff after its ready time promotes it back to active so a later
    // cycle can retry. This exercises the queue's exponential-backoff
    // promotion path end-to-end through the Scheduler.
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());
    src.add_node(node("tiny", 100, 256));
    src.add_pod(pod("huge", 5_000, 1));

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    // First attempt fails -> pod lands in backoff (queue len stays 1).
    sched.run_once().await.unwrap();
    assert!(sink.binds().is_empty());
    assert_eq!(sched.queue.len(), 1);
    // Nothing is active yet, so a second run_once finds an empty active queue.
    assert!(sched.run_once().await.unwrap().is_none());
    // Flush backoff well past the 1s initial window -> pod returns to active.
    sched.queue.flush_backoff(60_000);
    let outcome = sched.run_once().await.unwrap();
    assert!(outcome.is_some(), "flushed pod should be popped and retried");
    assert_eq!(outcome.unwrap().pod_full_name, "default/huge");
}

#[tokio::test]
async fn end_to_end_three_pods_drain_in_priority_order() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());
    src.add_node(node("n", 100_000, 1_000_000));
    for (i, prio) in [(1, 10), (2, 50), (3, 5)] {
        let mut p = pod(&format!("p{i}"), 10, 10);
        p.spec.priority = prio;
        src.add_pod(p);
    }
    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    let outcomes = sched.drain_active().await.unwrap();
    let bound_names: Vec<String> = sink
        .binds()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(bound_names.len(), 3);
    assert_eq!(bound_names[0], "default/p2"); // prio 50
    assert_eq!(bound_names[1], "default/p1"); // prio 10
    assert_eq!(bound_names[2], "default/p3"); // prio 5
    assert_eq!(outcomes.len(), 3);
}
