// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the event-driven scheduler loop against an in-memory
//! "mock apiserver" (`InMemorySource`/`InMemorySink`).
//!
//! These exercise the real `Scheduler::run` event loop end to end: pods and
//! nodes are streamed as watch events, the loop schedules and binds them, and a
//! later node-add wakes a previously-unschedulable pod — the behaviour that the
//! pre-loop `sync()`/`run_once()` driver could not provide.

use std::sync::Arc;
use std::time::Duration;

use cave_home_scheduler_rs::{
    InMemorySink, InMemorySource, Node, ObjectMeta, Pod, Quantity, ResourceName, Scheduler,
};
use tokio::sync::Notify;

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
    let mut c = cave_home_scheduler_rs::Container::default();
    c.resources
        .requests
        .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
    c.resources
        .requests
        .insert(ResourceName::Memory, Quantity::bytes(mem));
    p.spec.containers.push(c);
    p
}

/// Poll `cond` until it holds or a generous timeout elapses.
async fn eventually(cond: impl Fn() -> bool) -> bool {
    for _ in 0..400 {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cond()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_loop_binds_pod_streamed_after_start() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());
    src.add_node(node("n1", 1000, 1024));

    let sched = Arc::new(Scheduler::new(src.clone(), sink.clone()));
    let cancel = Arc::new(Notify::new());
    let run = tokio::spawn({
        let s = sched.clone();
        let c = cancel.clone();
        async move { s.run(c).await }
    });

    // The pod is published as a watch event *after* the loop is already running.
    src.add_pod(pod("alpha", 100, 256));

    assert!(
        eventually(|| !sink.binds().is_empty()).await,
        "pod should be bound by the running loop"
    );
    assert_eq!(sink.binds()[0], ("default/alpha".into(), "n1".into()));

    cancel.notify_waiters();
    let _ = run.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_loop_node_add_unblocks_pending_pod() {
    let src = Arc::new(InMemorySource::new());
    let sink = Arc::new(InMemorySink::new());
    // Pod arrives first, with NO node in the cluster — it must go unschedulable.
    src.add_pod(pod("beta", 100, 256));

    let sched = Arc::new(Scheduler::new(src.clone(), sink.clone()));
    let cancel = Arc::new(Notify::new());
    let run = tokio::spawn({
        let s = sched.clone();
        let c = cancel.clone();
        async move { s.run(c).await }
    });

    // Give the loop a chance to fail the pod and park it in unschedulable.
    assert!(
        eventually(|| sink
            .events()
            .iter()
            .any(|(_, reason, _)| reason == "FailedScheduling"))
        .await,
        "pod should have been attempted and recorded a FailedScheduling event"
    );
    assert!(sink.binds().is_empty(), "no node yet -> no bind");

    // A node now joins the cluster as a watch event — this must wake the pod.
    src.add_node(node("n2", 1000, 1024));

    assert!(
        eventually(|| !sink.binds().is_empty()).await,
        "node-add event should wake the pending pod and bind it"
    );
    assert_eq!(sink.binds()[0], ("default/beta".into(), "n2".into()));

    cancel.notify_waiters();
    let _ = run.await;
}
