// SPDX-License-Identifier: Apache-2.0
//! Decoupling boundary between the scheduler and the apiserver.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/scheduler.go::Scheduler {Client, Informers}
//!
//! Upstream goes through the typed client-go informers; here we expose
//! two narrow traits that the scheduler depends on. The apiserver crate
//! (when ready) will implement these traits via a shim layer so the
//! scheduler never depends on the apiserver crate directly.

use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::types::{Node, Pod};

/// Upstream: `pkg/scheduler/internal/queue/events.go::PodEvent`.
#[derive(Debug, Clone)]
pub enum PodEvent {
    Add(Pod),
    Update { old: Pod, new: Pod },
    Delete(Pod),
}

/// Concrete stream type used by [`SchedulerSource::watch_pods`].
/// A tokio mpsc receiver suffices — upstream uses a watch loop that
/// invokes the queue's `Add/Update/Delete` callbacks; the receiver
/// here serialises the same callbacks for a single subscriber.
pub type EventStream = mpsc::UnboundedReceiver<PodEvent>;

/// Upstream: the Node informer's `Add/Update/Delete` handlers
/// (`pkg/scheduler/eventhandlers.go::addNodeToCache` et al.).
#[derive(Debug, Clone)]
pub enum NodeEvent {
    Add(Node),
    Update { old: Node, new: Node },
    Delete(Node),
}

/// Concrete stream type used by [`SchedulerSource::watch_nodes`].
pub type NodeEventStream = mpsc::UnboundedReceiver<NodeEvent>;

/// Errors surfaced by source / sink implementations.
#[derive(Debug, thiserror::Error)]
pub enum SourceSinkError {
    #[error("apiserver: {0}")]
    Apiserver(String),
    #[error("pod {namespace}/{name} not found")]
    NotFound { namespace: String, name: String },
    #[error("conflict on bind for pod {namespace}/{name}")]
    Conflict { namespace: String, name: String },
}

pub type Result<T> = std::result::Result<T, SourceSinkError>;

/// Source of truth for what the scheduler should be reading.
///
/// Upstream: `pkg/scheduler/scheduler.go::Scheduler` (informer reads).
#[async_trait]
pub trait SchedulerSource: Send + Sync {
    /// Pods whose `.spec.nodeName` is empty and which target this profile.
    async fn list_pending_pods(&self) -> Result<Vec<Pod>>;
    /// All current nodes the scheduler can place onto.
    async fn list_nodes(&self) -> Result<Vec<Node>>;
    /// Streaming pod events (Add / Update / Delete).
    async fn watch_pods(&self) -> Result<EventStream>;
    /// Streaming node events (Add / Update / Delete). The event loop folds
    /// these into the cache and wakes pods waiting on node changes.
    async fn watch_nodes(&self) -> Result<NodeEventStream>;
}

/// Sink for scheduling decisions.
///
/// Upstream: `pkg/scheduler/scheduler.go::Scheduler.bind` +
/// `pkg/scheduler/scheduler.go::Scheduler.recorder`.
#[async_trait]
pub trait SchedulerSink: Send + Sync {
    /// Bind a pod to a node — upstream `core/v1.Binding` POST.
    async fn bind(&self, pod: &Pod, node_name: &str) -> Result<()>;
    /// Record an `Event` against the pod.
    async fn record_event(&self, pod: &Pod, reason: &str, message: &str) -> Result<()>;
}

// ---------- In-memory implementations (driven from tests + integrations) -----

/// In-memory `SchedulerSource` used by integration tests and the local
/// single-binary deployment path. Not a stub — fully implemented.
#[derive(Default, Clone)]
pub struct InMemorySource {
    inner: Arc<Mutex<InMemorySourceInner>>,
}

#[derive(Default)]
struct InMemorySourceInner {
    pods: Vec<Pod>,
    nodes: Vec<Node>,
    pending: VecDeque<PodEvent>,
    subscribers: Vec<mpsc::UnboundedSender<PodEvent>>,
    node_pending: VecDeque<NodeEvent>,
    node_subscribers: Vec<mpsc::UnboundedSender<NodeEvent>>,
}

impl InMemorySource {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_pod(&self, pod: Pod) {
        let mut g = self.inner.lock();
        g.pods.push(pod.clone());
        let ev = PodEvent::Add(pod);
        g.pending.push_back(ev.clone());
        g.subscribers.retain(|s| s.send(ev.clone()).is_ok());
    }

    pub fn add_node(&self, node: Node) {
        let mut g = self.inner.lock();
        g.nodes.push(node.clone());
        let ev = NodeEvent::Add(node);
        g.node_pending.push_back(ev.clone());
        g.node_subscribers.retain(|s| s.send(ev.clone()).is_ok());
    }

    pub fn delete_node(&self, name: &str) {
        let mut g = self.inner.lock();
        if let Some(idx) = g.nodes.iter().position(|n| n.metadata.name == name) {
            let n = g.nodes.remove(idx);
            let ev = NodeEvent::Delete(n);
            g.node_pending.push_back(ev.clone());
            g.node_subscribers.retain(|s| s.send(ev.clone()).is_ok());
        }
    }

    pub fn update_pod(&self, new: Pod) {
        let mut g = self.inner.lock();
        if let Some(slot) = g.pods.iter_mut().find(|p| p.metadata.uid == new.metadata.uid) {
            let old = slot.clone();
            *slot = new.clone();
            let ev = PodEvent::Update { old, new };
            g.pending.push_back(ev.clone());
            g.subscribers.retain(|s| s.send(ev.clone()).is_ok());
        }
    }

    pub fn delete_pod(&self, uid: &str) {
        let mut g = self.inner.lock();
        if let Some(idx) = g.pods.iter().position(|p| p.metadata.uid == uid) {
            let p = g.pods.remove(idx);
            let ev = PodEvent::Delete(p);
            g.pending.push_back(ev.clone());
            g.subscribers.retain(|s| s.send(ev.clone()).is_ok());
        }
    }

    /// Drain any pending events emitted since the last call.
    /// Used by the in-process `Scheduler::run_once` driver in tests.
    pub fn drain_pending(&self) -> Vec<PodEvent> {
        self.inner.lock().pending.drain(..).collect()
    }
}

#[async_trait]
impl SchedulerSource for InMemorySource {
    async fn list_pending_pods(&self) -> Result<Vec<Pod>> {
        let g = self.inner.lock();
        Ok(g.pods
            .iter()
            .filter(|p| p.spec.node_name.is_empty())
            .cloned()
            .collect())
    }

    async fn list_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.inner.lock().nodes.clone())
    }

    async fn watch_pods(&self) -> Result<EventStream> {
        let (tx, rx) = mpsc::unbounded_channel();
        // Replay the buffered backlog so new subscribers see the world.
        let backlog: Vec<PodEvent> = {
            let mut g = self.inner.lock();
            g.subscribers.push(tx.clone());
            g.pending.iter().cloned().collect()
        };
        for ev in backlog {
            // Bounded by current backlog; UnboundedSender::send only fails if
            // the receiver was dropped, which cannot happen yet.
            let _ = tx.send(ev);
        }
        Ok(rx)
    }

    async fn watch_nodes(&self) -> Result<NodeEventStream> {
        let (tx, rx) = mpsc::unbounded_channel();
        let backlog: Vec<NodeEvent> = {
            let mut g = self.inner.lock();
            g.node_subscribers.push(tx.clone());
            g.node_pending.iter().cloned().collect()
        };
        for ev in backlog {
            let _ = tx.send(ev);
        }
        Ok(rx)
    }
}

/// In-memory `SchedulerSink` that records every bind + event.
#[derive(Default, Clone)]
pub struct InMemorySink {
    inner: Arc<Mutex<InMemorySinkInner>>,
}

#[derive(Default, Debug)]
struct InMemorySinkInner {
    pub binds: Vec<(String, String)>,            // (pod full_name, node)
    pub events: Vec<(String, String, String)>,   // (pod full_name, reason, msg)
}

impl InMemorySink {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn binds(&self) -> Vec<(String, String)> {
        self.inner.lock().binds.clone()
    }

    #[must_use]
    pub fn events(&self) -> Vec<(String, String, String)> {
        self.inner.lock().events.clone()
    }
}

#[async_trait]
impl SchedulerSink for InMemorySink {
    async fn bind(&self, pod: &Pod, node_name: &str) -> Result<()> {
        self.inner
            .lock()
            .binds
            .push((pod.full_name(), node_name.to_string()));
        Ok(())
    }

    async fn record_event(&self, pod: &Pod, reason: &str, message: &str) -> Result<()> {
        self.inner.lock().events.push((
            pod.full_name(),
            reason.to_string(),
            message.to_string(),
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Node, ObjectMeta, Pod};

    fn pending_pod(ns: &str, n: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                namespace: ns.into(),
                name: n.into(),
                uid: format!("{ns}-{n}"),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn in_memory_source_filters_pending_pods() {
        let src = InMemorySource::new();
        src.add_pod(pending_pod("default", "alpha"));
        let mut bound = pending_pod("default", "beta");
        bound.spec.node_name = "n1".into();
        src.add_pod(bound);

        let pending = src.list_pending_pods().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].metadata.name, "alpha");
    }

    #[tokio::test]
    async fn in_memory_source_lists_nodes() {
        let src = InMemorySource::new();
        let mut n = Node::default();
        n.metadata.name = "n1".into();
        src.add_node(n);
        assert_eq!(src.list_nodes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn in_memory_sink_records_binds_and_events() {
        let sink = InMemorySink::new();
        let p = pending_pod("default", "alpha");
        sink.bind(&p, "n1").await.unwrap();
        sink.record_event(&p, "Scheduled", "Placed on n1").await.unwrap();
        assert_eq!(sink.binds(), vec![("default/alpha".into(), "n1".into())]);
        assert_eq!(
            sink.events(),
            vec![("default/alpha".into(), "Scheduled".into(), "Placed on n1".into())]
        );
    }

    #[tokio::test]
    async fn in_memory_source_emits_events_on_add() {
        let src = InMemorySource::new();
        let mut stream = src.watch_pods().await.unwrap();
        src.add_pod(pending_pod("default", "alpha"));
        let ev = stream.recv().await;
        assert!(matches!(ev, Some(PodEvent::Add(_))));
    }

    #[tokio::test]
    async fn in_memory_source_emits_delete_event() {
        let src = InMemorySource::new();
        src.add_pod(pending_pod("default", "alpha"));
        let mut stream = src.watch_pods().await.unwrap();
        // Drain the backlogged Add.
        let _ = stream.recv().await;
        src.delete_pod("default-alpha");
        let ev = stream.recv().await;
        assert!(matches!(ev, Some(PodEvent::Delete(_))));
    }
}
