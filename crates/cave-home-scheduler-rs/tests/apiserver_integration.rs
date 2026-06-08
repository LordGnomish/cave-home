// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Cross-crate integration: the scheduler's `SchedulerSource` / `SchedulerSink`
//! seam wired to the **real** `cave-home-apiserver-rs` `Registry`.
//!
//! This proves the seam is not a private fiction: an `ApiserverSource` reads
//! unscheduled Pods + Nodes out of the actual apiserver object store (the same
//! `Registry` the apiserver crate serves over REST), and an `ApiserverSink`
//! writes the scheduling decision back by mutating `spec.nodeName` on the Pod
//! object via the Registry's optimistic-concurrency `update` — the in-process
//! equivalent of POSTing a `core/v1.Binding`. After a scheduling cycle the test
//! reads the Pod *back out of the apiserver* and asserts it is now bound.
//!
//! The translation lives entirely in this test (the library stays
//! transport-agnostic): scheduler-typed `Pod`/`Node` ⇄ apiserver `json::Value`.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use cave_home_apiserver_rs::json::{obj, Value};
use cave_home_apiserver_rs::{GroupVersionResource, Registry};

use cave_home_scheduler_rs::source_sink::{
    EventStream, NodeEvent, NodeEventStream, PodEvent, Result, SchedulerSink, SchedulerSource,
    SourceSinkError,
};
use cave_home_scheduler_rs::{
    Container, Node, ObjectMeta, Pod, Quantity, ResourceName, Scheduler,
};

// ---------------------------------------------------------------------------
// Shared apiserver store + GVRs
// ---------------------------------------------------------------------------

type Store = Arc<Mutex<Registry>>;

fn pods_gvr() -> GroupVersionResource {
    GroupVersionResource::new("", "v1", "pods")
}
fn nodes_gvr() -> GroupVersionResource {
    GroupVersionResource::new("", "v1", "nodes")
}

// ---------------------------------------------------------------------------
// Typed <-> apiserver Value translation
// ---------------------------------------------------------------------------

/// Build a `core/v1.Pod` apiserver object with a single container requesting
/// `cpu_m` milliCPU. `node_name` empty = unscheduled.
fn pod_value(ns: &str, name: &str, cpu_m: i64) -> Value {
    let container = obj([
        ("name", Value::from("app")),
        (
            "resources",
            obj([("requests", obj([("cpu", Value::from(cpu_m))]))]),
        ),
    ]);
    obj([
        ("apiVersion", Value::from("v1")),
        ("kind", Value::from("Pod")),
        (
            "metadata",
            obj([("namespace", Value::from(ns)), ("name", Value::from(name))]),
        ),
        (
            "spec",
            obj([
                ("nodeName", Value::from("")),
                ("containers", Value::Array(vec![container])),
            ]),
        ),
    ])
}

/// Build a `core/v1.Node` apiserver object allocatable `cpu_m` milliCPU.
fn node_value(name: &str, cpu_m: i64) -> Value {
    obj([
        ("apiVersion", Value::from("v1")),
        ("kind", Value::from("Node")),
        ("metadata", obj([("name", Value::from(name))])),
        (
            "status",
            obj([("allocatable", obj([("cpu", Value::from(cpu_m))]))]),
        ),
    ])
}

/// apiserver Pod `Value` → scheduler-typed `Pod`.
fn value_to_pod(v: &Value) -> Pod {
    let mut p = Pod::default();
    p.metadata = ObjectMeta {
        namespace: v.pointer("metadata.namespace").and_then(Value::as_str).unwrap_or("").into(),
        name: v.pointer("metadata.name").and_then(Value::as_str).unwrap_or("").into(),
        // The apiserver stamps a uid on create; mirror it so the cache's
        // assumed-pod / forget bookkeeping keys correctly.
        uid: v.pointer("metadata.uid").and_then(Value::as_str).unwrap_or("").into(),
        ..Default::default()
    };
    p.spec.node_name = v.pointer("spec.nodeName").and_then(Value::as_str).unwrap_or("").into();
    if let Some(cpu) = v.pointer("spec.containers").and_then(Value::as_array).and_then(|cs| {
        cs.first()
            .and_then(|c| c.pointer("resources.requests.cpu"))
            .and_then(number_i64)
    }) {
        let mut c = Container::default();
        c.resources
            .requests
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
        p.spec.containers.push(c);
    }
    p
}

/// apiserver Node `Value` → scheduler-typed `Node`.
fn value_to_node(v: &Value) -> Node {
    let mut n = Node::default();
    n.metadata.name = v.pointer("metadata.name").and_then(Value::as_str).unwrap_or("").into();
    if let Some(cpu) = v.pointer("status.allocatable.cpu").and_then(number_i64) {
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
    }
    n
}

fn number_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(f) => Some(*f as i64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// SchedulerSource backed by the apiserver Registry
// ---------------------------------------------------------------------------

struct ApiserverSource {
    store: Store,
}

#[async_trait]
impl SchedulerSource for ApiserverSource {
    async fn list_pending_pods(&self) -> Result<Vec<Pod>> {
        let g = self.store.lock();
        let list = g
            .list(&pods_gvr(), &Default::default())
            .map_err(|e| SourceSinkError::Apiserver(format!("{e:?}")))?;
        Ok(list
            .items
            .iter()
            .map(value_to_pod)
            .filter(|p| p.spec.node_name.is_empty())
            .collect())
    }

    async fn list_nodes(&self) -> Result<Vec<Node>> {
        let g = self.store.lock();
        let list = g
            .list(&nodes_gvr(), &Default::default())
            .map_err(|e| SourceSinkError::Apiserver(format!("{e:?}")))?;
        Ok(list.items.iter().map(value_to_node).collect())
    }

    async fn watch_pods(&self) -> Result<EventStream> {
        // A real list-then-watch: replay the current Pod set as Add events. The
        // sync()/run_once() driver this test uses does not consume the stream,
        // but the contract is honoured (a live subscriber sees the world).
        let (tx, rx) = mpsc::unbounded_channel();
        for p in self.list_pending_pods().await? {
            let _ = tx.send(PodEvent::Add(p));
        }
        Ok(rx)
    }

    async fn watch_nodes(&self) -> Result<NodeEventStream> {
        let (tx, rx) = mpsc::unbounded_channel();
        for n in self.list_nodes().await? {
            let _ = tx.send(NodeEvent::Add(n));
        }
        Ok(rx)
    }
}

// ---------------------------------------------------------------------------
// SchedulerSink backed by the apiserver Registry
// ---------------------------------------------------------------------------

struct ApiserverSink {
    store: Store,
    events: Arc<Mutex<Vec<(String, String, String)>>>,
}

#[async_trait]
impl SchedulerSink for ApiserverSink {
    async fn bind(&self, pod: &Pod, node_name: &str) -> Result<()> {
        let mut g = self.store.lock();
        // GET the live object (carries the current resourceVersion + uid).
        let mut obj = g
            .get(&pods_gvr(), &pod.metadata.namespace, &pod.metadata.name)
            .map_err(|_| SourceSinkError::NotFound {
                namespace: pod.metadata.namespace.clone(),
                name: pod.metadata.name.clone(),
            })?;
        // Set spec.nodeName — the binding write-back (≈ POST core/v1.Binding).
        if let Some(spec) = obj.pointer_mut_object("spec") {
            spec.insert("nodeName".to_string(), Value::from(node_name));
        }
        // UPDATE with the optimistic-concurrency rv just read.
        g.update(&pods_gvr(), obj).map_err(|e| {
            // A concurrent writer would surface a Conflict here.
            SourceSinkError::Conflict {
                namespace: pod.metadata.namespace.clone(),
                name: format!("{}: {e:?}", pod.metadata.name),
            }
        })?;
        Ok(())
    }

    async fn record_event(&self, pod: &Pod, reason: &str, message: &str) -> Result<()> {
        self.events
            .lock()
            .push((pod.full_name(), reason.to_string(), message.to_string()));
        Ok(())
    }
}

/// Small extension: mutable object-field access on the apiserver `Value`.
trait PointerMut {
    fn pointer_mut_object(&mut self, key: &str) -> Option<&mut std::collections::BTreeMap<String, Value>>;
}
impl PointerMut for Value {
    fn pointer_mut_object(&mut self, key: &str) -> Option<&mut std::collections::BTreeMap<String, Value>> {
        match self {
            Value::Object(m) => m.get_mut(key).and_then(|v| match v {
                Value::Object(inner) => Some(inner),
                _ => None,
            }),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn seed_store() -> Store {
    let mut reg = Registry::new();
    // A node with 4000m CPU and one unscheduled pod requesting 500m.
    reg.create(&nodes_gvr(), node_value("n1", 4000)).unwrap();
    reg.create(&pods_gvr(), pod_value("default", "alpha", 500))
        .unwrap();
    Arc::new(Mutex::new(reg))
}

#[tokio::test]
async fn scheduler_reads_and_binds_through_the_real_apiserver_registry() {
    let store = seed_store();
    let src = Arc::new(ApiserverSource { store: store.clone() });
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(ApiserverSink {
        store: store.clone(),
        events: events.clone(),
    });

    let sched = Scheduler::new(src.clone(), sink.clone());
    sched.sync().await.unwrap();
    let outcome = sched.run_once().await.unwrap().expect("a pod was scheduled");
    assert_eq!(outcome.pod_full_name, "default/alpha");

    // Read the Pod back OUT of the apiserver: the binding must be persisted.
    let bound = store
        .lock()
        .get(&pods_gvr(), "default", "alpha")
        .expect("pod still in apiserver");
    assert_eq!(
        bound.pointer("spec.nodeName").and_then(Value::as_str),
        Some("n1"),
        "scheduler must have written spec.nodeName back through the apiserver"
    );

    // A Scheduled event was recorded against the pod.
    assert!(events
        .lock()
        .iter()
        .any(|(p, r, _)| p == "default/alpha" && r == "Scheduled"));
}

#[tokio::test]
async fn apiserver_round_trip_lists_only_unscheduled_pods() {
    let store = seed_store();
    // Add a second pod that is ALREADY bound — it must not appear as pending.
    {
        let mut g = store.lock();
        let mut v = pod_value("default", "beta", 100);
        if let Value::Object(ref mut m) = v {
            if let Some(Value::Object(spec)) = m.get_mut("spec") {
                spec.insert("nodeName".to_string(), Value::from("n1"));
            }
        }
        g.create(&pods_gvr(), v).unwrap();
    }
    let src = ApiserverSource { store: store.clone() };
    let pending = src.list_pending_pods().await.unwrap();
    let names: Vec<&str> = pending.iter().map(|p| p.metadata.name.as_str()).collect();
    assert_eq!(names, vec!["alpha"], "only the unscheduled pod is pending");

    let nodes = src.list_nodes().await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].metadata.name, "n1");
}
