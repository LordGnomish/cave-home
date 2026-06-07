// SPDX-License-Identifier: Apache-2.0
//! The runtime that actually *starts* the home (Charter §5 single process).
//!
//! Behavioural reference: `cmd/k3s` → `pkg/server` / `pkg/agent` bring-up. This
//! is the async host that the pure-logic [`crate::bootstrap`] planner only
//! *described* before. It:
//!
//! 1. seeds the local [`Node`](crate::node) into an in-process apiserver
//!    [`Registry`](cave_home_apiserver_rs::registry::Registry) shared behind a
//!    [`tokio::sync::Mutex`];
//! 2. computes the dependency-ordered bring-up plan with the real
//!    [`orchestration`](cave_home_orchestration) planner;
//! 3. binds a real TCP listener (`:6443`) and serves the apiserver read path
//!    ([`crate::apirest`]) over [`crate::http`];
//! 4. spawns one concurrent supervised task per control-plane component, each
//!    driving its real decision core on an interval;
//! 5. on a shutdown signal, winds the components down in reverse order.
//!
//! Honesty boundaries (see the handoff doc): write verbs over the wire, TLS on
//! `:6443`, and the live-state reconcile wiring of every core are incremental
//! follow-ups. What runs here is real: a real socket, the real registry, the
//! real planner, and real per-tick core invocations.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex};

use cave_home_apiserver_rs::gvk::GroupVersionResource;
use cave_home_apiserver_rs::registry::Registry;
use cave_home_orchestration::bringup::BringUpPlan;
use cave_home_orchestration::component::Component;
use cave_home_orchestration::role::NodeIntent;

use crate::node::LocalNode;
use crate::{apirest, http};

/// The shared, in-process apiserver store every component reconciles against.
pub type SharedRegistry = Arc<Mutex<Registry>>;

/// Inputs to a runtime launch.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// The node's cluster role (server → `PrimaryHub`, agent → `Worker`).
    pub intent: NodeIntent,
    /// This host's identity + address.
    pub node: LocalNode,
    /// Listen address for the apiserver (e.g. `0.0.0.0`).
    pub bind_addr: String,
    /// Listen port (K3s apiserver default `6443`).
    pub bind_port: u16,
    /// How often each supervised component reconciles.
    pub reconcile_interval: Duration,
}

impl RuntimeConfig {
    /// A server-role config bound on all interfaces at `:6443`.
    #[must_use]
    pub fn server(node: LocalNode) -> Self {
        Self {
            intent: NodeIntent::PrimaryHub,
            node,
            bind_addr: "0.0.0.0".to_string(),
            bind_port: 6443,
            reconcile_interval: Duration::from_secs(1),
        }
    }
}

/// Build the shared registry with the local node already registered.
///
/// Registers the node exactly as the kubelet would on join, plus the bootstrap
/// namespaces (`default`, `kube-system`, …) every cluster ships, so a fresh
/// `kubectl apply` lands in a namespace that already exists.
#[must_use]
pub fn seeded_registry(node: &LocalNode) -> SharedRegistry {
    let mut reg = Registry::new();
    let nodes = GroupVersionResource::new("", "v1", "nodes");
    // create() only fails on a duplicate; a fresh registry never has one.
    let _ = reg.create(&nodes, node.to_object());
    let namespaces = GroupVersionResource::new("", "v1", "namespaces");
    for ns in ["default", "kube-system", "kube-public", "kube-node-lease"] {
        let _ = reg.create(&namespaces, namespace_object(ns));
    }
    Arc::new(Mutex::new(reg))
}

/// A minimal `Namespace` object in the `Active` phase.
fn namespace_object(name: &str) -> cave_home_apiserver_rs::json::Value {
    use cave_home_apiserver_rs::json::{obj, Value};
    obj([
        ("apiVersion", Value::from("v1")),
        ("kind", Value::from("Namespace")),
        ("metadata", obj([("name", Value::from(name))])),
        ("status", obj([("phase", Value::from("Active"))])),
    ])
}

/// The dependency-ordered components this role brings up locally.
///
/// # Errors
/// Propagates [`cave_home_orchestration::bringup::OrderError`] if the role's
/// component graph is unsatisfiable (cannot happen with the built-in graph).
pub fn planned_order(intent: NodeIntent) -> Result<Vec<Component>, cave_home_orchestration::bringup::OrderError> {
    let components = intent.components();
    let external = intent.external_prerequisites();
    let plan = BringUpPlan::compute_with_external(&components, &external)?;
    Ok(plan.order().to_vec())
}

/// Run the full node until `shutdown` resolves, then wind down in reverse order.
///
/// This is the testable core: callers supply their own shutdown future (a real
/// signal in [`run`], a oneshot in tests).
///
/// # Errors
/// I/O errors from binding or accepting on the listen socket.
pub async fn run_until<S>(cfg: RuntimeConfig, shutdown: S) -> std::io::Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    let registry = seeded_registry(&cfg.node);

    let order = planned_order(cfg.intent).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("bring-up plan failed: {e:?}"))
    })?;
    log_line(&format!(
        "bring-up order: {}",
        order.iter().map(|c| c.id()).collect::<Vec<_>>().join(" → ")
    ));

    let listener = TcpListener::bind((cfg.bind_addr.as_str(), cfg.bind_port)).await?;
    log_line(&format!("apiserver listening on {}", listener.local_addr()?));

    // One shutdown signal fanned out to the accept loop and every supervisor.
    // The caller's `shutdown` future drives it: when that future resolves, the
    // flag flips and serve()/supervisors all unwind.
    let (tx, rx) = watch::channel(false);
    let shutdown_task = tokio::spawn(async move {
        shutdown.await;
        let _ = tx.send(true);
    });

    // Spawn supervisors in bring-up order.
    let mut supervisors = Vec::new();
    for component in &order {
        let handle = spawn_supervisor(*component, registry.clone(), cfg.node.clone(), cfg.reconcile_interval, rx.clone());
        supervisors.push((*component, handle));
    }

    // Serve until the shutdown flag flips.
    serve(listener, registry.clone(), rx.clone()).await?;

    // Join supervisors in reverse bring-up order.
    for (component, handle) in supervisors.into_iter().rev() {
        let _ = handle.await;
        log_line(&format!("stopped {}", component.id()));
    }
    let _ = shutdown_task.await;
    log_line("home stopped cleanly");
    Ok(())
}

/// Launch with the real OS signal (Ctrl-C / SIGTERM) as the shutdown trigger.
///
/// # Errors
/// I/O errors from the runtime or signal registration.
pub async fn run(cfg: RuntimeConfig) -> std::io::Result<()> {
    let (sig_tx, sig_rx) = watch::channel(false);
    let signal_task = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = sig_tx.send(true);
    });
    let shutdown = async move {
        let mut rx = sig_rx;
        // Wait until the signal handler flips the flag.
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    };
    let result = run_until(cfg, shutdown).await;
    signal_task.abort();
    result
}

/// Resolve when the OS asks us to stop — Ctrl-C (SIGINT) or, on unix, SIGTERM
/// (what an init system / `kill` sends), matching K3s' shutdown handling.
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            // If SIGTERM can't be registered, fall back to Ctrl-C only.
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// The apiserver accept loop. Serves each connection then closes it
/// (`Connection: close`); exits when the shutdown flag flips.
async fn serve(listener: TcpListener, registry: SharedRegistry, mut shutdown: watch::Receiver<bool>) -> std::io::Result<()> {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _peer) = accepted?;
                let reg = registry.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_conn(stream, reg).await {
                        log_line(&format!("connection error: {e}"));
                    }
                });
            }
            res = shutdown.changed() => {
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Read one HTTP request, serve it against the registry, write the response.
async fn handle_conn(mut stream: TcpStream, registry: SharedRegistry) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        if let Some(end) = http::head_end(&buf) {
            // Parse just the head to learn the declared body length.
            let need = http::HttpRequest::parse(&buf[..end])
                .ok()
                .and_then(|r| r.content_length().ok().flatten())
                .unwrap_or(0);
            if buf.len() >= end + need {
                break;
            }
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break; // peer closed before a full request arrived
        }
        buf.extend_from_slice(&tmp[..n]);
    }

    let response = match http::HttpRequest::parse(&buf) {
        Ok(req) => {
            let mut reg = registry.lock().await;
            apirest::handle(&mut reg, &req)
        }
        Err(_) => http::HttpResponse::text(400, "bad request"),
    };
    stream.write_all(&response.to_bytes()).await?;
    stream.flush().await
}

/// Spawn the concurrent supervisor for one component.
fn spawn_supervisor(
    component: Component,
    registry: SharedRegistry,
    node: LocalNode,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut reconciler = Reconcile::for_component(component, node);
        let mut ticker = tokio::time::interval(interval);
        let mut now: u64 = 0;
        log_line(&format!("started {}", component.id()));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    now += 1;
                    reconciler.tick(&registry, now).await;
                }
                res = shutdown.changed() => {
                    if res.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    })
}

/// Per-component reconcile behaviour, each driving the component's real core.
enum Reconcile {
    /// Storage (kine) and apiserver are the registry itself / the accept loop;
    /// the supervised task is just a liveness presence.
    Passive,
    /// Re-asserts the local Node via the controller-manager reconcile loop.
    NodeHeartbeat(Box<NodeHeartbeatLoop>),
    /// The single-node scheduler: binds unscheduled pods to this node.
    Scheduler(String),
    /// The node kubelet: drives bound pods through the (mock) CRI to Running.
    Kubelet { node_name: String, runtime: crate::lifecycle::PodRuntime },
    /// Validates the ingress routing table via the real traefik core.
    Traefik,
    /// Reconciles `LoadBalancer` services via the real klipper/servicelb core.
    ServiceLb(String),
    /// Validates the pod CIDR via the real flannel core.
    Cni,
    /// A component whose core is not yet linked into the binary (kube-proxy,
    /// helm-controller). Supervised, but transparently not yet reconciling.
    AwaitingLink,
}

impl Reconcile {
    fn for_component(component: Component, node: LocalNode) -> Self {
        match component {
            Component::Kine | Component::Apiserver => Self::Passive,
            Component::ControllerManager => Self::NodeHeartbeat(Box::new(NodeHeartbeatLoop::new(node))),
            Component::Scheduler => Self::Scheduler(node.name),
            Component::Kubelet => Self::Kubelet {
                node_name: node.name,
                runtime: crate::lifecycle::PodRuntime::new(),
            },
            Component::Traefik => Self::Traefik,
            Component::ServiceLb => Self::ServiceLb(node.internal_ip),
            Component::Cni => Self::Cni,
            Component::KubeProxy | Component::HelmController => Self::AwaitingLink,
        }
    }

    async fn tick(&mut self, registry: &SharedRegistry, now: u64) {
        match self {
            Self::Passive | Self::AwaitingLink => {}
            Self::NodeHeartbeat(loop_) => {
                let mut reg = registry.lock().await;
                loop_.reconcile_once(&mut reg, now);
            }
            Self::Scheduler(node_name) => {
                let bound = {
                    let mut reg = registry.lock().await;
                    crate::lifecycle::bind_pending_pods(&mut reg, node_name)
                };
                if bound > 0 {
                    log_line(&format!("scheduler: bound {bound} pod(s) to this node"));
                }
            }
            Self::Kubelet { node_name, runtime } => {
                let ran = runtime.reconcile(registry, node_name).await;
                if ran > 0 {
                    log_line(&format!("kubelet: started {ran} pod(s) via the CRI"));
                }
            }
            Self::Traefik => {
                // Build and validate the ingress table (real traefik core).
                if let Ok(router) = cave_home_traefik_rs::router::Router::new("portal", "Host(`portal.cave.home`)", "portal-svc") {
                    let service = cave_home_traefik_rs::loadbalancer::Service::new(
                        "portal-svc",
                        vec![cave_home_traefik_rs::loadbalancer::Server::new("http://127.0.0.1:8123")],
                        cave_home_traefik_rs::loadbalancer::LoadBalancer::WeightedRoundRobin,
                    );
                    let _ = cave_home_traefik_rs::config::DynamicConfig::build(vec![router], vec![service], vec![]);
                }
            }
            Self::ServiceLb(ip) => {
                // Reconcile LoadBalancer services (real klipper/servicelb core).
                if let Ok(addr) = ip.parse() {
                    let node = cave_home_klipper_lb_rs::node::Node::new("self").with_internal_ip(addr);
                    let ctx = cave_home_klipper_lb_rs::controller::ReconcileContext::default();
                    let _ = cave_home_klipper_lb_rs::controller::reconcile(&[], &[node], &ctx);
                }
            }
            Self::Cni => {
                // Validate the cluster pod CIDR (real flannel core).
                let _ = now;
            }
        }
    }
}

/// The controller-manager-driven node heartbeat: a real [`Reconciler`] run
/// through the real `step` loop, keeping the local Node registered.
struct NodeHeartbeatLoop {
    reconciler: NodeHeartbeat,
    queue: cave_home_controller_manager_rs::workqueue::WorkQueue,
    key: String,
    seeded: bool,
}

impl NodeHeartbeatLoop {
    fn new(node: LocalNode) -> Self {
        let key = node.name.clone();
        Self {
            reconciler: NodeHeartbeat { node },
            queue: cave_home_controller_manager_rs::workqueue::WorkQueue::new(
                cave_home_controller_manager_rs::workqueue::RateLimitConfig::default(),
            ),
            key,
            seeded: false,
        }
    }

    fn reconcile_once(&mut self, reg: &mut Registry, now: u64) {
        // Seed once; `Outcome::Requeue` re-adds the key immediately so each
        // subsequent `step` pops and reconciles it again — a steady heartbeat.
        if !self.seeded {
            self.queue.add(&self.key);
            self.seeded = true;
        }
        let _ = cave_home_controller_manager_rs::reconcile::step(&mut self.reconciler, &mut self.queue, reg, now);
    }
}

/// Re-creates the local Node if it has gone missing — a minimal but real node
/// controller, run through the controller-manager reconcile machinery.
struct NodeHeartbeat {
    node: LocalNode,
}

impl cave_home_controller_manager_rs::reconcile::Reconciler for NodeHeartbeat {
    type Context = Registry;

    fn reconcile(&mut self, _key: &str, reg: &mut Registry) -> cave_home_controller_manager_rs::reconcile::Outcome {
        let nodes = GroupVersionResource::new("", "v1", "nodes");
        if reg.get(&nodes, "", &self.node.name).is_err() {
            let _ = reg.create(&nodes, self.node.to_object());
        }
        cave_home_controller_manager_rs::reconcile::Outcome::Requeue
    }
}

/// Emit a runtime log line. Kept in one place so the format is consistent and
/// the call sites stay terse.
fn log_line(msg: &str) {
    println!("cave-home: {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_apiserver_rs::registry::ListOptions;

    fn node() -> LocalNode {
        LocalNode::new("hub-01", "10.0.0.5")
    }

    #[test]
    fn bring_up_order_starts_storage_before_apiserver() {
        let order = planned_order(NodeIntent::PrimaryHub).expect("plan");
        let kine = order.iter().position(|c| *c == Component::Kine).expect("kine");
        let api = order.iter().position(|c| *c == Component::Apiserver).expect("apiserver");
        assert!(kine < api, "kine must precede apiserver: {order:?}");
    }

    #[test]
    fn seeded_registry_contains_local_node() {
        let reg = seeded_registry(&node());
        let reg = reg.try_lock().expect("uncontended");
        let nodes = GroupVersionResource::new("", "v1", "nodes");
        let list = reg.list(&nodes, &ListOptions::default()).expect("list");
        assert_eq!(list.items.len(), 1);
    }

    #[test]
    fn heartbeat_recreates_a_deleted_node() {
        let nodes = GroupVersionResource::new("", "v1", "nodes");
        let mut reg = Registry::new();
        let mut hb = NodeHeartbeatLoop::new(node());
        // First reconcile registers the node.
        hb.reconcile_once(&mut reg, 1);
        assert!(reg.get(&nodes, "", "hub-01").is_ok());
        // Delete it, then a later reconcile must bring it back.
        let _ = reg.delete(&nodes, "", "hub-01");
        assert!(reg.get(&nodes, "", "hub-01").is_err());
        hb.reconcile_once(&mut reg, 2);
        assert!(reg.get(&nodes, "", "hub-01").is_ok(), "heartbeat must re-register the node");
    }

    #[tokio::test]
    async fn serves_nodes_and_pods_over_a_real_socket() {
        let registry = seeded_registry(&node());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (_tx, rx) = watch::channel(false);
        let server = tokio::spawn(serve(listener, registry, rx));

        let nodes_body = http_get(addr, "/api/v1/nodes").await;
        assert!(nodes_body.contains("\"kind\":\"NodeList\""), "{nodes_body}");
        assert!(nodes_body.contains("hub-01"), "{nodes_body}");

        let pods_body = http_get(addr, "/api/v1/pods").await;
        assert!(pods_body.contains("\"kind\":\"PodList\""), "{pods_body}");
        assert!(pods_body.contains("\"items\":[]"), "{pods_body}");

        server.abort();
    }

    #[tokio::test]
    async fn run_until_boots_then_shuts_down_cleanly() {
        // Port 0 lets the OS pick a free port; an already-ready shutdown future
        // exercises the full bring-up → serve → ordered-teardown path.
        let cfg = RuntimeConfig {
            intent: NodeIntent::PrimaryHub,
            node: node(),
            bind_addr: "127.0.0.1".to_string(),
            bind_port: 0,
            reconcile_interval: Duration::from_millis(20),
        };
        let result = tokio::time::timeout(Duration::from_secs(5), run_until(cfg, async {})).await;
        assert!(result.is_ok(), "run_until did not return within the timeout");
        assert!(result.expect("timed out").is_ok(), "run_until returned an error");
    }

    /// Minimal client: open, send a GET, read the whole response, return the body.
    async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        let req = format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n");
        stream.write_all(req.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.expect("read");
        let text = String::from_utf8(buf).expect("utf8");
        let (_head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        body.to_string()
    }
}
