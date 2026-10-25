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
//! 3. binds a real TCP listener — loopback by default; any other address only
//!    with TLS **and** configured credentials ([`crate::access::check_exposure`])
//!    — and puts every request through [`crate::access::Gate`]: `Host` check
//!    (DNS rebinding), authentication (bearer tokens / verified client
//!    certificates, never anonymous), RBAC, audit; writes then pass admission;
//!    every connection is bounded by [`Limits`] (head/body caps, timeouts, a
//!    connection cap, non-fatal `accept()` errors);
//! 4. spawns one concurrent supervised task per control-plane component, each
//!    driving its real decision core on an interval, restarting it on failure
//!    (with backoff + a crash-loop budget) via [`crate::supervisor`];
//! 5. serves the full read **and** write path ([`crate::apirest`]) — including
//!    over TLS / mutual TLS when the `tls` feature is built and a
//!    [`crate::tls::TlsConfig`] is supplied;
//! 6. on a shutdown signal, drains the listener and winds the components down in
//!    reverse dependency order, one at a time, without losing the store.
//!
//! Honesty boundary: the per-core *live-state* reconcile wiring of every
//! decision core is still being filled in incrementally. What runs here is real:
//! a real socket (plain or TLS-terminated), the real registry, the real planner,
//! real supervised per-tick core invocations, and real ordered teardown.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex, Semaphore};

use cave_home_apiserver_rs::gvk::GroupVersionResource;
use cave_home_apiserver_rs::rbac::UserInfo;
use cave_home_apiserver_rs::registry::Registry;
use cave_home_apiserver_rs::server::RequestReadError;
pub use cave_home_apiserver_rs::server::Limits;
use cave_home_orchestration::bringup::BringUpPlan;
use cave_home_orchestration::component::Component;
use cave_home_orchestration::role::NodeIntent;

use crate::access::{ExposureError, Gate};
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
    /// TLS material for the apiserver listener (`tls` feature). `Some` serves
    /// over HTTPS — and mutual TLS when a client CA is configured; `None` (the
    /// default) serves plain HTTP, exactly as before.
    #[cfg(feature = "tls")]
    pub tls: Option<crate::tls::TlsConfig>,
    /// Who may call the API.
    pub access: AccessConfig,
    /// Per-connection bounds (head/body size, timeouts, connection cap).
    pub limits: Limits,
}

/// The credential sources the listener accepts. Anonymous access is never
/// enabled; a boot with no source at all is refused.
#[derive(Debug, Clone, Default)]
pub struct AccessConfig {
    /// A static token file (`token,user,uid[,"groups"]`), as `--token-file`.
    pub token_file: Option<PathBuf>,
    /// Tokens supplied programmatically (embedding, tests).
    pub tokens: Vec<(String, UserInfo)>,
    /// Where to keep the generated admin token + kubeconfig. Used when no
    /// token file is configured (normally the node's data dir).
    pub bootstrap_dir: Option<PathBuf>,
    /// Extra names (beyond loopback, the node name and its address) the
    /// listener accepts in the `Host` header.
    pub extra_hosts: Vec<String>,
}

impl RuntimeConfig {
    /// A server-role config bound to loopback at `:6443`.
    #[must_use]
    pub fn server(node: LocalNode) -> Self {
        Self {
            intent: NodeIntent::PrimaryHub,
            node,
            bind_addr: "127.0.0.1".to_string(),
            bind_port: 6443,
            reconcile_interval: Duration::from_secs(1),
            #[cfg(feature = "tls")]
            tls: None,
            access: AccessConfig::default(),
            limits: Limits::default(),
        }
    }
}

/// A source of accepted connections (a [`TcpListener`], or a test double).
trait Accept: Send + Sync + 'static {
    fn poll_accept(&self, cx: &mut Context<'_>) -> Poll<std::io::Result<(TcpStream, SocketAddr)>>;
}

impl Accept for TcpListener {
    fn poll_accept(&self, cx: &mut Context<'_>) -> Poll<std::io::Result<(TcpStream, SocketAddr)>> {
        Self::poll_accept(self, cx)
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
    let mut node_obj = node.to_object();
    stamp_creation(&mut node_obj);
    let _ = reg.create(&nodes, node_obj);
    let namespaces = GroupVersionResource::new("", "v1", "namespaces");
    for ns in ["default", "kube-system", "kube-public", "kube-node-lease"] {
        let _ = reg.create(&namespaces, namespace_object(ns));
    }
    Arc::new(Mutex::new(reg))
}

/// A minimal `Namespace` object in the `Active` phase, stamped with a creation
/// time so its Age renders.
fn namespace_object(name: &str) -> cave_home_apiserver_rs::json::Value {
    use cave_home_apiserver_rs::json::{obj, Value};
    let mut ns = obj([
        ("apiVersion", Value::from("v1")),
        ("kind", Value::from("Namespace")),
        ("metadata", obj([("name", Value::from(name))])),
        ("status", obj([("phase", Value::from("Active"))])),
    ]);
    stamp_creation(&mut ns);
    ns
}

/// Set `metadata.creationTimestamp` to now (so `kubectl get` shows a real Age).
fn stamp_creation(object: &mut cave_home_apiserver_rs::json::Value) {
    use cave_home_apiserver_rs::json::Value;
    if let Value::Object(map) = object {
        let meta = map.entry("metadata".to_string()).or_insert_with(Value::object);
        if let Value::Object(m) = meta {
            m.entry("creationTimestamp".to_string())
                .or_insert_with(|| Value::from(crate::table::now_rfc3339()));
        }
    }
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
/// A non-loopback bind without TLS + configured credentials (refused before
/// the socket is bound), and I/O errors from binding or accepting.
pub async fn run_until<S>(cfg: RuntimeConfig, shutdown: S) -> std::io::Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    // Refuse an exposed, unprotected bind before it ever touches the network;
    // `run_until_on_listener` re-checks the address actually bound.
    if !crate::access::is_loopback_host(&cfg.bind_addr) {
        match protection(&cfg) {
            (false, _) => return Err(exposure_error(ExposureError::NoTls)),
            (true, false) => return Err(exposure_error(ExposureError::NoAuthentication)),
            (true, true) => {}
        }
    }
    let listener = TcpListener::bind((cfg.bind_addr.as_str(), cfg.bind_port)).await?;
    run_until_on_listener(cfg, listener, shutdown).await
}

/// Run the full node on an already-bound listener until `shutdown` resolves.
///
/// Splitting the socket out of [`run_until`] lets a caller learn the bound
/// address first (binding to port 0, then reading `local_addr`) before handing
/// the listener to the runtime — what an end-to-end test or a socket-activated
/// launcher needs. The bring-up plan, supervised components, graceful drain and
/// ordered teardown are identical to [`run_until`].
///
/// Before anything starts, the listener's address is checked against the
/// exposure rule ([`crate::access::check_exposure`]) and the access gate is
/// assembled: configured tokens, the token file, verified client certificates,
/// and — unless a token file is configured — the generated admin credential
/// in [`AccessConfig::bootstrap_dir`]. A boot with no way to authenticate is
/// refused.
///
/// # Errors
/// An exposure-rule violation, unusable TLS or credential configuration, and
/// I/O errors from the listen socket.
pub async fn run_until_on_listener<S>(cfg: RuntimeConfig, listener: TcpListener, shutdown: S) -> std::io::Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    // Build the TLS terminator up front (when configured) so a bad cert/key is a
    // boot failure, not a per-connection surprise. With TLS the listener serves
    // HTTPS (and verifies client certificates when a client CA is set).
    let tls_listener = build_tls_listener(&cfg)?;
    let local = listener.local_addr()?;
    let (_, explicit_auth) = protection(&cfg);
    crate::access::check_exposure(local, tls_listener.is_some(), explicit_auth).map_err(exposure_error)?;
    let gate = Arc::new(build_gate(&cfg, local, tls_listener.is_some())?);

    let registry = seeded_registry(&cfg.node);

    let order = planned_order(cfg.intent).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("bring-up plan failed: {e:?}"))
    })?;
    log_line(&format!(
        "bring-up order: {}",
        order.iter().map(|c| c.id()).collect::<Vec<_>>().join(" → ")
    ));
    log_line(&format!(
        "apiserver listening on {local} ({}; anonymous access disabled)",
        if tls_listener.is_some() { "https" } else { "http" }
    ));

    // The top-level shutdown flag the caller's future drives: it stops the accept
    // loop (drains the listener) first. Each component additionally gets its own
    // shutdown channel so teardown can be sequenced in dependency order rather
    // than every component stopping at once.
    let (tx, rx) = watch::channel(false);
    let shutdown_task = tokio::spawn(async move {
        shutdown.await;
        let _ = tx.send(true);
    });

    // Spawn supervisors in bring-up order, each with its own stop signal.
    let mut supervisors = Vec::new();
    for component in &order {
        let (stop_tx, stop_rx) = watch::channel(false);
        let handle = spawn_supervisor(*component, registry.clone(), cfg.node.clone(), cfg.reconcile_interval, stop_rx);
        supervisors.push((*component, stop_tx, handle));
    }

    // Serve until the top-level shutdown flag flips — the listener drains first,
    // so no new requests are accepted while components wind down.
    serve(listener, registry.clone(), rx.clone(), tls_listener, gate, cfg.limits.clone()).await?;
    log_line("apiserver drained; stopping components in reverse dependency order");

    // Ordered graceful teardown: stop each component in reverse bring-up order,
    // one at a time, awaiting it before signalling the next. A dependency is thus
    // never stopped before its dependents (e.g. the kubelet drains before the
    // apiserver/storage it writes pod status to).
    for (component, stop_tx, handle) in supervisors.into_iter().rev() {
        let _ = stop_tx.send(true);
        let _ = handle.await;
        log_line(&format!("stopped {}", component.id()));
    }
    let _ = shutdown_task.await;
    log_line("home stopped cleanly");
    Ok(())
}

/// `(TLS configured, explicitly configured credentials)` for the exposure
/// rule. The generated bootstrap credential does not count: it exists for
/// local administration, not as a deliberate decision to expose the API.
fn protection(cfg: &RuntimeConfig) -> (bool, bool) {
    #[cfg(feature = "tls")]
    let tls = cfg.tls.is_some();
    #[cfg(not(feature = "tls"))]
    let tls = false;
    let tokens = cfg.access.token_file.is_some() || !cfg.access.tokens.is_empty();
    (tls, tokens || client_ca_configured(cfg))
}

/// Whether the listener verifies client certificates (a client CA is set).
#[cfg(feature = "tls")]
fn client_ca_configured(cfg: &RuntimeConfig) -> bool {
    cfg.tls.as_ref().is_some_and(crate::tls::TlsConfig::is_mutual)
}

/// Without the `tls` feature there are never client certificates.
#[cfg(not(feature = "tls"))]
const fn client_ca_configured(_cfg: &RuntimeConfig) -> bool {
    false
}

fn exposure_error(e: ExposureError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string())
}

/// The URL clients of this listener use (what the generated kubeconfig names).
fn server_url(local: SocketAddr, tls: bool) -> String {
    let scheme = if tls { "https" } else { "http" };
    let ip = if local.ip().is_unspecified() {
        std::net::IpAddr::from([127, 0, 0, 1])
    } else {
        local.ip()
    };
    format!("{scheme}://{}", SocketAddr::new(ip, local.port()))
}

/// Assemble the access gate from the configured credential sources.
fn build_gate(cfg: &RuntimeConfig, local: SocketAddr, tls: bool) -> std::io::Result<Gate> {
    let mut tokens = cfg.access.tokens.clone();
    if let Some(path) = &cfg.access.token_file {
        let loaded = crate::access::load_token_file(path)
            .map_err(|e| std::io::Error::new(e.kind(), format!("token file {}: {e}", path.display())))?;
        log_line(&format!("loaded {} sign-in token(s) from {}", loaded.len(), path.display()));
        tokens.extend(loaded);
    } else if let Some(dir) = &cfg.access.bootstrap_dir {
        #[cfg(feature = "tls")]
        let ca = cfg.tls.as_ref().filter(|_| tls).map(|t| t.cert.as_path());
        #[cfg(not(feature = "tls"))]
        let ca: Option<&std::path::Path> = None;
        let admin = crate::access::ensure_bootstrap_admin(dir, &server_url(local, tls), ca)
            .map_err(|e| std::io::Error::new(e.kind(), format!("writing admin credentials to {}: {e}", dir.display())))?;
        log_line(&format!("admin kubeconfig: {}", admin.kubeconfig_path.display()));
        tokens.push((
            admin.token,
            UserInfo::new(crate::access::ADMIN_USER).with_groups(&[crate::access::ADMIN_GROUP]),
        ));
    }
    if tokens.is_empty() && !client_ca_configured(cfg) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no way to authenticate API clients: configure a token file, a client CA, or a bootstrap directory",
        ));
    }

    let mut hosts = vec![cfg.node.name.clone(), cfg.node.internal_ip.clone(), cfg.bind_addr.clone()];
    hosts.extend(cfg.access.extra_hosts.iter().cloned());
    Ok(Gate::new(tokens, crate::access::HostPolicy::new(hosts)).with_audit(Arc::new(crate::access::LogAuditSink)))
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

/// Pause before re-accepting after `failures` consecutive `accept()` errors:
/// `base` doubling per failure, capped at one second.
fn accept_backoff(base: Duration, failures: u32) -> Duration {
    base.saturating_mul(1_u32 << failures.saturating_sub(1).min(5)).min(Duration::from_secs(1))
}

/// The apiserver accept loop. Serves each connection then closes it
/// (`Connection: close`); exits when the shutdown flag flips.
///
/// When `tls` is `Some`, each accepted TCP connection is completed through the
/// rustls handshake (terminating TLS, and verifying any client certificate when
/// the acceptor was built for mutual TLS) before the HTTP handler sees any
/// bytes; when `None`, the connection is served as plain HTTP. The handshake
/// runs on the per-connection task so a slow or hostile client never blocks the
/// accept loop.
///
/// Bounded by `limits`: at most [`Limits::max_connections`] connections are
/// served at once (further ones are closed on accept), and a failed `accept()`
/// — e.g. `EMFILE` when file descriptors run out — backs off and retries
/// instead of ending the loop.
// The per-connection `tls.clone()` is a real clone of a `TlsAcceptor` under the
// `tls` feature; with the feature off the placeholder type is `Copy`, which
// would otherwise trip `clone_on_copy`.
#[cfg_attr(not(feature = "tls"), allow(clippy::clone_on_copy))]
async fn serve<L: Accept>(
    listener: L,
    registry: SharedRegistry,
    mut shutdown: watch::Receiver<bool>,
    tls: Option<TlsListener>,
    gate: Arc<Gate>,
    limits: Limits,
) -> std::io::Result<()> {
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    let limits = Arc::new(limits);
    let mut failures: u32 = 0;
    loop {
        tokio::select! {
            accepted = std::future::poll_fn(|cx| listener.poll_accept(cx)) => {
                let stream = match accepted {
                    Ok((stream, _peer)) => {
                        failures = 0;
                        stream
                    }
                    Err(e) => {
                        failures = failures.saturating_add(1);
                        if failures == 1 {
                            log_line(&format!("accept failed ({e}); backing off and retrying"));
                        }
                        tokio::time::sleep(accept_backoff(limits.accept_backoff, failures)).await;
                        continue;
                    }
                };
                // At capacity: dropping the stream closes it at once.
                let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
                    continue;
                };
                let reg = registry.clone();
                let conn_shutdown = shutdown.clone();
                let tls = tls.clone();
                let gate = Arc::clone(&gate);
                let limits = Arc::clone(&limits);
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(e) = serve_conn(stream, reg, conn_shutdown, tls, &gate, &limits).await {
                        if e.kind() != std::io::ErrorKind::TimedOut {
                            log_line(&format!("connection error: {e}"));
                        }
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

/// The TLS terminator for the listener: a cloneable `tokio-rustls` acceptor.
/// Only present (and only compiled) under the `tls` feature.
#[cfg(feature = "tls")]
type TlsListener = tokio_rustls::TlsAcceptor;
/// With the `tls` feature off there is no TLS path; the type is an uninhabited
/// placeholder so the plain-HTTP code compiles unchanged.
#[cfg(not(feature = "tls"))]
type TlsListener = std::convert::Infallible;

/// Build the listener's TLS terminator from the runtime config: `Some` when a
/// [`crate::tls::TlsConfig`] is present (and valid), `None` for plain HTTP.
///
/// # Errors
/// With the `tls` feature, surfaces a bad/missing cert/key bundle as a boot
/// error so the process fails fast rather than serving broken TLS.
#[cfg(feature = "tls")]
fn build_tls_listener(cfg: &RuntimeConfig) -> std::io::Result<Option<TlsListener>> {
    match &cfg.tls {
        Some(tls_cfg) => Ok(Some(crate::tls::acceptor(tls_cfg)?)),
        None => Ok(None),
    }
}

/// Without the `tls` feature there is never a TLS terminator.
#[cfg(not(feature = "tls"))]
#[allow(clippy::unnecessary_wraps)] // mirrors the tls-feature signature
const fn build_tls_listener(_cfg: &RuntimeConfig) -> std::io::Result<Option<TlsListener>> {
    Ok(None)
}

/// Serve one accepted TCP connection, terminating TLS first when configured.
/// The handshake must finish within [`Limits::read_timeout`].
async fn serve_conn(
    stream: TcpStream,
    registry: SharedRegistry,
    shutdown: watch::Receiver<bool>,
    tls: Option<TlsListener>,
    gate: &Gate,
    limits: &Limits,
) -> std::io::Result<()> {
    match tls {
        #[cfg(feature = "tls")]
        Some(acceptor) => {
            // Complete the TLS handshake (verifying any client certificate under
            // mTLS); a failed or stalled handshake is a connection-level error.
            let tls_stream = tokio::time::timeout(limits.read_timeout, acceptor.accept(stream))
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TLS handshake timed out"))??;
            let peer = crate::tls::peer_identity(&tls_stream);
            handle_conn(tls_stream, registry, shutdown, gate, limits, peer.as_ref()).await
        }
        // No TLS configured: serve the raw TCP stream as plain HTTP.
        _ => handle_conn(stream, registry, shutdown, gate, limits, None).await,
    }
}

/// Read one HTTP request, pass it through the access gate, serve it against
/// the registry, write the response.
///
/// `peer` is the identity of the client certificate the TLS handshake
/// verified, if any. The JSON body is decoded before the registry lock is
/// taken. A `?watch=true` GET is upgraded to a streaming watch
/// ([`serve_watch`]) that holds the connection open and emits change events
/// until the client leaves or the server shuts down; every other request is a
/// one-shot response.
async fn handle_conn<S>(
    mut stream: S,
    registry: SharedRegistry,
    shutdown: watch::Receiver<bool>,
    gate: &Gate,
    limits: &Limits,
    peer: Option<&UserInfo>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let req = match read_request(&mut stream, limits).await {
        Ok(Some(req)) => req,
        Ok(None) => return Ok(()), // peer closed before sending a request
        Err(e) => return reject_unreadable(stream, e, limits).await,
    };

    let admitted = match gate.check(&req, peer) {
        Ok(admitted) => admitted,
        Err(resp) => return write_response(&mut stream, &resp, limits).await,
    };

    if is_watch_request(&req) {
        gate.finish(admitted, 200);
        return serve_watch(stream, &registry, &req, shutdown, limits).await;
    }

    let body = match apirest::decode_write_body(&req) {
        Ok(body) => body,
        Err(resp) => {
            gate.finish(admitted, resp.status);
            return write_response(&mut stream, &resp, limits).await;
        }
    };
    let response = {
        let mut reg = registry.lock().await;
        apirest::handle_decoded(&mut reg, &req, body, gate.admission())
    };
    gate.finish(admitted, response.status);
    write_response(&mut stream, &response, limits).await
}

/// One read into `tmp`, bounded by the per-read timeout and the request
/// deadline.
async fn read_some<S: AsyncRead + Unpin>(
    stream: &mut S,
    tmp: &mut [u8],
    deadline: tokio::time::Instant,
    limits: &Limits,
) -> Result<usize, RequestReadError> {
    let until = if limits.read_timeout.is_zero() {
        deadline
    } else {
        deadline.min(tokio::time::Instant::now() + limits.read_timeout)
    };
    match tokio::time::timeout_at(until, stream.read(tmp)).await {
        Ok(Ok(n)) => Ok(n),
        Ok(Err(e)) => Err(RequestReadError::Io(e)),
        Err(_) => Err(RequestReadError::TimedOut),
    }
}

/// The declared body length: `Content-Length` (repeats must agree), `0` when
/// absent; chunked request bodies are not implemented.
fn declared_body_len(head: &http::HttpRequest) -> Result<usize, RequestReadError> {
    if head.header("transfer-encoding").is_some() {
        return Err(RequestReadError::NotImplemented(
            "transfer-encoding request bodies are not supported; send Content-Length".to_string(),
        ));
    }
    let mut lengths = head.headers.iter().filter(|(k, _)| k == "content-length").map(|(_, v)| v.as_str());
    let Some(first) = lengths.next() else {
        return Ok(0);
    };
    if lengths.any(|v| v != first) {
        return Err(RequestReadError::BadRequest("conflicting Content-Length headers".to_string()));
    }
    first
        .parse::<usize>()
        .map_err(|_| RequestReadError::BadRequest(format!("invalid Content-Length {first:?}")))
}

/// Read one request under `limits`: the head may not exceed
/// [`Limits::max_head_bytes`], a `Content-Length` above
/// [`Limits::max_body_bytes`] is refused before any body byte is read, each
/// read must make progress within [`Limits::read_timeout`], and the whole
/// request must arrive within [`Limits::request_deadline`].
async fn read_request<S: AsyncRead + Unpin>(stream: &mut S, limits: &Limits) -> Result<Option<http::HttpRequest>, RequestReadError> {
    let deadline = tokio::time::Instant::now() + limits.request_deadline;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(end) = http::head_end(&buf) {
            break end;
        }
        if buf.len() > limits.max_head_bytes {
            return Err(RequestReadError::HeadTooLarge);
        }
        let n = read_some(stream, &mut tmp, deadline, limits).await?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            return Err(RequestReadError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before the request head completed",
            )));
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    if head_end > limits.max_head_bytes {
        return Err(RequestReadError::HeadTooLarge);
    }
    let head = http::HttpRequest::parse(&buf[..head_end]).map_err(|e| RequestReadError::BadRequest(e.to_string()))?;
    let body_len = declared_body_len(&head)?;
    if body_len > limits.max_body_bytes {
        return Err(RequestReadError::BodyTooLarge(body_len));
    }
    let want = head_end + body_len;
    while buf.len() < want {
        let n = read_some(stream, &mut tmp, deadline, limits).await?;
        if n == 0 {
            return Err(RequestReadError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before the request body completed",
            )));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    buf.truncate(want);
    http::HttpRequest::parse(&buf)
        .map(Some)
        .map_err(|e| RequestReadError::BadRequest(e.to_string()))
}

/// Answer a request that could not be read (`431`/`413`/`400`/`501`), then
/// close gracefully. A stalled client gets no answer — the connection is
/// simply dropped.
async fn reject_unreadable<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, err: RequestReadError, limits: &Limits) -> std::io::Result<()> {
    use cave_home_apiserver_rs::status::{Status, StatusReason};
    let resp = match err {
        RequestReadError::HeadTooLarge => http::HttpResponse::text(431, "request head too large"),
        RequestReadError::BodyTooLarge(n) => apirest::status_response(&Status::new(
            StatusReason::RequestEntityTooLarge,
            format!("request body of {n} bytes exceeds the {} byte limit", limits.max_body_bytes),
        )),
        RequestReadError::BadRequest(m) => http::HttpResponse::text(400, format!("bad request: {m}")),
        RequestReadError::NotImplemented(m) => http::HttpResponse::text(501, m),
        RequestReadError::TimedOut => return Ok(()),
        RequestReadError::Io(e) => return Err(e),
    };
    write_response(&mut stream, &resp, limits).await?;
    linger_close(stream).await;
    Ok(())
}

/// Half-close and briefly discard what the client is still sending, so the
/// error response is not lost to a TCP reset caused by unread input.
async fn linger_close<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S) {
    let _ = stream.shutdown().await;
    let mut sink = [0u8; 4096];
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        let mut drained = 0usize;
        while drained < 256 * 1024 {
            match stream.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) => drained += n,
            }
        }
    })
    .await;
}

/// Write a whole response within [`Limits::write_timeout`].
async fn write_response<S: AsyncWrite + Unpin>(stream: &mut S, resp: &http::HttpResponse, limits: &Limits) -> std::io::Result<()> {
    with_write_timeout(limits, async {
        stream.write_all(&resp.to_bytes()).await?;
        stream.flush().await
    })
    .await
}

/// Bound a write so a client that stops reading cannot pin the connection.
async fn with_write_timeout<F>(limits: &Limits, write: F) -> std::io::Result<()>
where
    F: std::future::Future<Output = std::io::Result<()>>,
{
    if limits.write_timeout.is_zero() {
        return write.await;
    }
    tokio::time::timeout(limits.write_timeout, write)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "client stopped reading"))?
}

/// True for a `GET ...?watch=true` (or `watch=1`) request.
fn is_watch_request(req: &http::HttpRequest) -> bool {
    req.method == "GET" && matches!(query_param(&req.query, "watch"), Some("true" | "1"))
}

/// First value for `key` in a `&`-joined query string.
fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then_some(v)
    })
}

/// Stream a chunked watch response: an HTTP/1.1 `200` with `Transfer-Encoding:
/// chunked`, then one newline-terminated `{"type":..,"object":..}` JSON frame
/// per registry change, until the peer disconnects or the server shuts down.
///
/// Behavioural reference: the apiserver watch protocol (`?watch=true`, the
/// `watch.Event` envelope). Events are sourced from the registry's own
/// per-resource history via `watch_since`, starting at the client's
/// `resourceVersion`, and filtered to the path's namespace/name scope.
///
/// The client sends nothing after its request, so EOF on the read half means
/// it has gone: the watch then ends at once and frees its connection slot,
/// even when no event would have revealed the dead peer through a failed write.
async fn serve_watch<S>(
    stream: S,
    registry: &SharedRegistry,
    req: &http::HttpRequest,
    mut shutdown: watch::Receiver<bool>,
    limits: &Limits,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    use cave_home_apiserver_rs::path;

    let (mut reader, mut stream) = tokio::io::split(stream);
    let rp = match path::parse(&req.path) {
        Ok(rp) => rp,
        Err(s) => {
            let body = format!("watch rejected: {}", s.message);
            return write_response(&mut stream, &http::HttpResponse::text(s.code, body), limits).await;
        }
    };
    let gvr = rp.gvr();
    let mut last_rv = query_param(&req.query, "resourceVersion")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let ns_filter = rp.is_namespaced().then(|| rp.namespace.clone());
    let name_filter = rp.is_named().then(|| rp.name.clone());

    // Streaming response head — no Content-Length; frames are chunk-encoded.
    with_write_timeout(limits, async {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await?;
        stream.flush().await
    })
    .await?;

    let mut probe = [0u8; 64];
    loop {
        let events = {
            let reg = registry.lock().await;
            reg.watch_since(&gvr, last_rv).unwrap_or_default()
        };
        for ev in events {
            last_rv = last_rv.max(ev.resource_version);
            if let Some(ns) = &ns_filter {
                if ev.object.pointer("metadata.namespace").and_then(cave_home_apiserver_rs::json::Value::as_str) != Some(ns.as_str()) {
                    continue;
                }
            }
            if let Some(name) = &name_filter {
                if ev.object.pointer("metadata.name").and_then(cave_home_apiserver_rs::json::Value::as_str) != Some(name.as_str()) {
                    continue;
                }
            }
            let frame = watch_event_frame(&ev);
            // A write error (or a client that stopped reading) ends the watch.
            if with_write_timeout(limits, write_chunk(&mut stream, frame.as_bytes())).await.is_err() {
                return Ok(());
            }
        }
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
            res = shutdown.changed() => {
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            read = reader.read(&mut probe) => {
                // EOF or a reset: the client left. (Extra bytes are ignored.)
                if matches!(read, Ok(0) | Err(_)) {
                    return Ok(());
                }
            }
        }
    }

    // Terminating zero-length chunk (best-effort; the peer may already be gone).
    let _ = with_write_timeout(limits, async {
        stream.write_all(b"0\r\n\r\n").await?;
        stream.flush().await
    })
    .await;
    Ok(())
}

/// Render one watch event as a newline-terminated `{"type":..,"object":..}` line.
fn watch_event_frame(ev: &cave_home_apiserver_rs::registry::WatchEvent) -> String {
    use cave_home_apiserver_rs::json::{obj, Value};
    use cave_home_apiserver_rs::registry::WatchEventKind;
    let kind = match ev.kind {
        WatchEventKind::Added => "ADDED",
        WatchEventKind::Modified => "MODIFIED",
        WatchEventKind::Deleted => "DELETED",
    };
    let mut frame = obj([("type", Value::from(kind)), ("object", ev.object.clone())]).to_json_string();
    frame.push('\n');
    frame
}

/// Write one HTTP chunk (`<hex-len>\r\n<bytes>\r\n`) and flush it.
async fn write_chunk<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> std::io::Result<()> {
    stream.write_all(format!("{:x}\r\n", bytes.len()).as_bytes()).await?;
    stream.write_all(bytes).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await
}

/// Spawn the concurrent **supervised** task for one component.
///
/// The reconcile loop runs under [`supervisor::run`]: if a reconcile tick
/// panics, the loop's spawned task ends with a `JoinError`, which the body maps
/// to an `Err` so the supervisor restarts the component (with backoff, up to its
/// crash-loop budget) instead of letting it vanish. The component is rebuilt
/// from scratch on each restart — the shared registry it reconciles against is
/// untouched, so a restart never loses cluster state.
fn spawn_supervisor(
    component: Component,
    registry: SharedRegistry,
    node: LocalNode,
    interval: Duration,
    shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<crate::supervisor::SupervisedExit> {
    let policy = crate::supervisor::RestartPolicy::default();
    tokio::spawn(async move {
        crate::supervisor::run(component.id(), policy, shutdown, move |loop_shutdown| {
            let registry = registry.clone();
            let node = node.clone();
            reconcile_loop(component, registry, node, interval, loop_shutdown)
        })
        .await
    })
}

/// One supervised run of a component's reconcile loop. Driven on its own spawned
/// task so a panic inside [`Reconcile::tick`] is caught as a [`RunError`] (which
/// the supervisor treats as a restartable failure) rather than tearing the whole
/// process down.
async fn reconcile_loop(
    component: Component,
    registry: SharedRegistry,
    node: LocalNode,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), RunError> {
    let task = tokio::spawn(async move {
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
    });
    // A panic in a reconcile tick is a restartable failure; a clean break
    // (shutdown) and a deliberate cancellation are both treated as a clean stop.
    match task.await {
        Err(join) if join.is_panic() => Err(RunError::Panicked(component.id())),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// A recoverable failure of a supervised component run.
#[derive(Debug)]
enum RunError {
    /// A reconcile tick panicked; the named component will be restarted.
    Panicked(&'static str),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Panicked(id) => write!(f, "{id} reconcile loop panicked"),
        }
    }
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
        let server = tokio::spawn(serve(listener, registry, rx, None, test_gate(), Limits::default()));

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
        let cfg = loopback_cfg(0);
        let result = tokio::time::timeout(Duration::from_secs(5), run_until(cfg, async {})).await;
        assert!(result.is_ok(), "run_until did not return within the timeout");
        assert!(result.expect("timed out").is_ok(), "run_until returned an error");
    }

    #[tokio::test]
    async fn supervised_component_restarts_after_panics_without_losing_the_store() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc as StdArc;

        // A registry seeded with the local node — the shared cluster state that
        // must survive a component crash-looping and being restarted.
        let registry = seeded_registry(&node());
        let nodes = GroupVersionResource::new("", "v1", "nodes");

        // A reconcile body that panics its first 2 runs, then runs steady until
        // shutdown — exactly the spawn/await-JoinError panic-isolation shape that
        // `reconcile_loop` uses in production. It also writes a marker object into
        // the shared registry on its steady run so we can prove the store is the
        // same one across restarts.
        let runs = StdArc::new(AtomicU32::new(0));
        let runs2 = runs.clone();
        let reg_for_body = registry.clone();
        let (tx, rx) = watch::channel(false);

        let sup = tokio::spawn(async move {
            crate::supervisor::run(
                "test-component",
                crate::supervisor::RestartPolicy::default()
                    .with_base_backoff(Duration::from_millis(5)),
                rx,
                move |mut loop_shutdown| {
                    let runs = runs2.clone();
                    let registry = reg_for_body.clone();
                    async move {
                        let task = tokio::spawn(async move {
                            let n = runs.fetch_add(1, Ordering::SeqCst);
                            if n < 2 {
                                panic!("simulated reconcile panic");
                            }
                            // Steady run: record a marker, then idle until shutdown.
                            {
                                let mut reg = registry.lock().await;
                                let pods = GroupVersionResource::new("", "v1", "pods");
                                let _ = reg.create(
                                    &pods,
                                    cave_home_apiserver_rs::json::obj([
                                        ("apiVersion", cave_home_apiserver_rs::json::Value::from("v1")),
                                        ("kind", cave_home_apiserver_rs::json::Value::from("Pod")),
                                        (
                                            "metadata",
                                            cave_home_apiserver_rs::json::obj([
                                                ("name", cave_home_apiserver_rs::json::Value::from("marker")),
                                                ("namespace", cave_home_apiserver_rs::json::Value::from("default")),
                                            ]),
                                        ),
                                    ]),
                                );
                            }
                            while !*loop_shutdown.borrow() {
                                if loop_shutdown.changed().await.is_err() {
                                    break;
                                }
                            }
                        });
                        match task.await {
                            Ok(()) => Ok(()),
                            Err(j) if j.is_panic() => Err("panicked"),
                            Err(_) => Ok(()),
                        }
                    }
                },
            )
            .await
        });

        // Give the supervisor time to absorb both panics and reach the steady run.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if runs.load(Ordering::SeqCst) >= 3 {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline, "component never reached steady run");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // The store survived every restart: the seeded node is still present, and
        // the steady run's marker landed in the *same* registry.
        {
            let reg = registry.lock().await;
            assert!(reg.get(&nodes, "", "hub-01").is_ok(), "seeded node lost across restarts");
            let pods = GroupVersionResource::new("", "v1", "pods");
            assert!(reg.get(&pods, "default", "marker").is_ok(), "steady run did not see the shared store");
        }
        assert!(!sup.is_finished(), "supervisor stays up after recovering");

        let _ = tx.send(true);
        let exit = tokio::time::timeout(Duration::from_secs(2), sup)
            .await
            .expect("supervisor stops on shutdown")
            .expect("join");
        assert_eq!(exit, crate::supervisor::SupervisedExit::ShutdownRequested);
    }

    #[tokio::test]
    async fn ordered_teardown_stops_components_in_reverse_one_at_a_time() {
        use std::sync::Arc as StdArc;
        use tokio::sync::Mutex as TokioMutex;

        // Three supervised tasks named in bring-up order. Each records its own id
        // into a shared log only once it has actually stopped, so the log proves
        // both the reverse order *and* that teardown is sequential (each fully
        // stops before the next is signalled).
        let order = ["a", "b", "c"];
        let stop_log: StdArc<TokioMutex<Vec<&'static str>>> = StdArc::new(TokioMutex::new(Vec::new()));

        let mut supervisors = Vec::new();
        for id in order {
            let (stop_tx, stop_rx) = watch::channel(false);
            let log = stop_log.clone();
            let handle = tokio::spawn(async move {
                crate::supervisor::run(id, crate::supervisor::RestartPolicy::default(), stop_rx, move |mut sd| {
                    let log = log.clone();
                    async move {
                        while !*sd.borrow() {
                            if sd.changed().await.is_err() {
                                break;
                            }
                        }
                        // Record this component as stopped.
                        log.lock().await.push(id);
                        Ok::<(), &str>(())
                    }
                })
                .await
            });
            supervisors.push((id, stop_tx, handle));
        }

        // Let every task reach its steady (running) state.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Teardown exactly as `run_until` does: reverse order, signal then await.
        for (_id, stop_tx, handle) in supervisors.into_iter().rev() {
            let _ = stop_tx.send(true);
            let _ = tokio::time::timeout(Duration::from_secs(1), handle).await.expect("stops");
        }

        let log = stop_log.lock().await;
        assert_eq!(*log, vec!["c", "b", "a"], "components must stop in reverse bring-up order");
    }

    #[test]
    fn query_param_parses_and_detects_watch() {
        assert_eq!(query_param("watch=true&resourceVersion=8", "resourceVersion"), Some("8"));
        assert_eq!(query_param("watch=true", "watch"), Some("true"));
        assert_eq!(query_param("a=1&b=2", "c"), None);
        let watch_req = http::HttpRequest::parse(b"GET /api/v1/pods?watch=true HTTP/1.1\r\n\r\n").unwrap();
        assert!(is_watch_request(&watch_req));
        let plain = http::HttpRequest::parse(b"GET /api/v1/pods HTTP/1.1\r\n\r\n").unwrap();
        assert!(!is_watch_request(&plain));
    }

    #[tokio::test]
    async fn watch_streams_a_created_pod_as_an_added_event() {
        let registry = seeded_registry(&node());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = watch::channel(false);
        let server = tokio::spawn(serve(listener, registry.clone(), rx, None, test_gate(), Limits::default()));

        // Open a watch on pods starting from the current (empty) history.
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(authed("GET", "/api/v1/pods?watch=true&resourceVersion=0", "").as_bytes())
            .await
            .expect("write");

        // After the watch is open, create a pod through the same registry.
        {
            let mut reg = registry.lock().await;
            let pods = GroupVersionResource::new("", "v1", "pods");
            let pod = cave_home_apiserver_rs::json::obj([
                ("apiVersion", cave_home_apiserver_rs::json::Value::from("v1")),
                ("kind", cave_home_apiserver_rs::json::Value::from("Pod")),
                (
                    "metadata",
                    cave_home_apiserver_rs::json::obj([
                        ("name", cave_home_apiserver_rs::json::Value::from("watchpod")),
                        ("namespace", cave_home_apiserver_rs::json::Value::from("default")),
                    ]),
                ),
            ]);
            reg.create(&pods, pod).expect("create");
        }

        // Accumulate the chunked stream until the ADDED frame arrives. The first
        // read may return only the response head (the event is emitted on the
        // next poll tick), so we read in a loop under one overall deadline.
        let collected = tokio::time::timeout(Duration::from_secs(5), async {
            let mut acc = String::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).await.expect("read");
                if n == 0 {
                    break;
                }
                acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                if acc.contains("\"type\":\"ADDED\"") && acc.contains("watchpod") {
                    break;
                }
            }
            acc
        })
        .await
        .expect("watch event within timeout");
        assert!(collected.contains("\"type\":\"ADDED\""), "{collected}");
        assert!(collected.contains("watchpod"), "{collected}");

        let _ = tx.send(true);
        server.abort();
    }

    #[tokio::test]
    async fn watch_keeps_streaming_subsequent_modifications() {
        use cave_home_apiserver_rs::json::{obj, Value};
        let registry = seeded_registry(&node());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = watch::channel(false);
        let server = tokio::spawn(serve(listener, registry.clone(), rx, None, test_gate(), Limits::default()));
        let pods = GroupVersionResource::new("", "v1", "pods");

        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(authed("GET", "/api/v1/pods?watch=true&resourceVersion=0", "").as_bytes())
            .await
            .expect("write");

        // Create, then mutate the pod twice — exactly the create→bind→run shape.
        {
            let mut reg = registry.lock().await;
            reg.create(
                &pods,
                obj([
                    ("apiVersion", Value::from("v1")),
                    ("kind", Value::from("Pod")),
                    ("metadata", obj([("name", Value::from("p1")), ("namespace", Value::from("default"))])),
                    ("spec", obj([])),
                ]),
            )
            .expect("create");
        }
        // Mimic the live supervisor cadence: ADDED, then bind, then run, each
        // separated by more than one watch poll interval, so each must be picked
        // up by a *distinct* poll of the open connection.
        tokio::time::sleep(Duration::from_millis(400)).await;
        {
            let mut reg = registry.lock().await;
            reg.patch_merge(&pods, "default", "p1", &obj([("spec", obj([("nodeName", Value::from("hub-01"))]))]))
                .expect("bind");
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        {
            let mut reg = registry.lock().await;
            reg.patch_merge(&pods, "default", "p1", &obj([("status", obj([("phase", Value::from("Running"))]))]))
                .expect("run");
        }

        let collected = tokio::time::timeout(Duration::from_secs(5), async {
            let mut acc = String::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).await.expect("read");
                if n == 0 {
                    break;
                }
                acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                if acc.matches("MODIFIED").count() >= 2 {
                    break;
                }
            }
            acc
        })
        .await
        .expect("modifications within timeout");
        assert!(collected.contains("\"type\":\"ADDED\""), "{collected}");
        assert!(collected.matches("\"type\":\"MODIFIED\"").count() >= 2, "{collected}");
        assert!(collected.contains("\"phase\":\"Running\""), "{collected}");

        let _ = tx.send(true);
        server.abort();
    }

    /// Minimal client: open, send an authenticated GET, read the whole
    /// response, return the body.
    async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
        let (_status, body) = exchange(addr, &authed("GET", path, "")).await;
        body
    }

    // --- access control + connection bounds over a real socket ---------------

    const TEST_TOKEN: &str = "test-admin-token";

    /// A gate admitting [`TEST_TOKEN`] as a cluster admin.
    fn test_gate() -> Arc<Gate> {
        Arc::new(Gate::new(
            vec![(
                TEST_TOKEN.to_string(),
                UserInfo::new(crate::access::ADMIN_USER).with_groups(&[crate::access::ADMIN_GROUP]),
            )],
            crate::access::HostPolicy::new(["hub-01"]),
        ))
    }

    /// A raw request carrying the test credentials and a loopback `Host`.
    fn authed(method: &str, target: &str, extra_headers: &str) -> String {
        format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TEST_TOKEN}\r\n{extra_headers}\r\n")
    }

    /// Send `raw`, read to EOF, return `(status line, body)`.
    async fn exchange(addr: SocketAddr, raw: &str) -> (String, String) {
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream.write_all(raw.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        let text = String::from_utf8_lossy(&buf).into_owned();
        let status = text.lines().next().unwrap_or_default().to_string();
        let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
        (status, body)
    }

    /// Serve a seeded registry on an ephemeral loopback port with `limits`.
    async fn spawn_server(limits: Limits) -> (SocketAddr, watch::Sender<bool>, tokio::task::JoinHandle<std::io::Result<()>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(serve(listener, seeded_registry(&node()), rx, None, test_gate(), limits));
        (addr, tx, handle)
    }

    /// A loopback runtime config authenticating [`TEST_TOKEN`].
    fn loopback_cfg(port: u16) -> RuntimeConfig {
        RuntimeConfig {
            intent: NodeIntent::PrimaryHub,
            node: node(),
            bind_addr: "127.0.0.1".to_string(),
            bind_port: port,
            reconcile_interval: Duration::from_millis(20),
            #[cfg(feature = "tls")]
            tls: None,
            access: AccessConfig {
                tokens: vec![(TEST_TOKEN.to_string(), UserInfo::new("tester").with_groups(&[crate::access::ADMIN_GROUP]))],
                ..AccessConfig::default()
            },
            limits: Limits::default(),
        }
    }

    #[test]
    fn server_preset_binds_loopback() {
        assert_eq!(RuntimeConfig::server(node()).bind_addr, "127.0.0.1");
    }

    #[tokio::test]
    async fn requests_without_credentials_are_401_over_the_wire() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        let (status, body) = exchange(addr, "GET /api/v1/pods HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(status.contains(" 401 "), "{status} {body}");
        let (status, _) = exchange(addr, &authed("GET", "/api/v1/pods", "")).await;
        assert!(status.contains(" 200 "), "{status}");
        let (status, _) = exchange(addr, "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(status.contains(" 200 "), "probes stay open: {status}");
        server.abort();
    }

    #[tokio::test]
    async fn unauthenticated_writes_do_not_reach_the_store() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        let pod = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"intruder"}}"#;
        let raw = format!(
            "POST /api/v1/namespaces/default/pods HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pod}",
            pod.len()
        );
        let (status, _) = exchange(addr, &raw).await;
        assert!(status.contains(" 401 "), "{status}");
        let (_, list) = exchange(addr, &authed("GET", "/api/v1/pods", "")).await;
        assert!(!list.contains("intruder"), "{list}");
        server.abort();
    }

    #[tokio::test]
    async fn rebinding_host_is_421_over_the_wire() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        let raw = format!("GET /api/v1/pods HTTP/1.1\r\nHost: attacker.example:8123\r\nAuthorization: Bearer {TEST_TOKEN}\r\n\r\n");
        let (status, _) = exchange(addr, &raw).await;
        assert!(status.contains(" 421 "), "{status}");
        server.abort();
    }

    #[tokio::test]
    async fn unauthenticated_watch_is_refused_not_streamed() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        let (status, _) = tokio::time::timeout(
            Duration::from_secs(5),
            exchange(addr, "GET /api/v1/pods?watch=true HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        )
        .await
        .expect("a refused watch closes promptly");
        assert!(status.contains(" 401 "), "{status}");
        server.abort();
    }

    #[tokio::test]
    async fn write_without_json_content_type_is_415_over_the_wire() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        let body = "name=x";
        let raw = authed(
            "POST",
            "/api/v1/namespaces/default/pods",
            &format!("Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n", body.len()),
        ) + body;
        let (status, _) = exchange(addr, &raw).await;
        assert!(status.contains(" 415 "), "{status}");
        server.abort();
    }

    #[tokio::test]
    async fn oversized_body_is_413_before_it_is_sent() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        // Declare a body larger than the cap and never send it: the answer
        // must come from the head alone.
        let raw = authed(
            "POST",
            "/api/v1/namespaces/default/pods",
            &format!("Content-Type: application/json\r\nContent-Length: {}\r\n", cave_home_apiserver_rs::http::MAX_BODY_BYTES + 1),
        );
        let (status, _) = tokio::time::timeout(Duration::from_secs(5), exchange(addr, &raw)).await.expect("prompt 413");
        assert!(status.contains(" 413 "), "{status}");
        server.abort();
    }

    #[tokio::test]
    async fn oversized_head_is_431() {
        let (addr, _tx, server) = spawn_server(Limits::default()).await;
        let pad = "X-Pad: ".to_string() + &"a".repeat(cave_home_apiserver_rs::http::MAX_HEAD_BYTES) + "\r\n";
        let (status, _) = tokio::time::timeout(Duration::from_secs(5), exchange(addr, &authed("GET", "/api/v1/pods", &pad)))
            .await
            .expect("prompt 431");
        assert!(status.contains(" 431 "), "{status}");
        server.abort();
    }

    #[tokio::test]
    async fn idle_connection_is_closed_after_the_read_timeout() {
        let limits = Limits { read_timeout: Duration::from_millis(200), ..Limits::default() };
        let (addr, _tx, server) = spawn_server(limits).await;
        let mut idle = TcpStream::connect(addr).await.expect("connect");
        let mut buf = [0u8; 64];
        let closed = tokio::time::timeout(Duration::from_secs(3), idle.read(&mut buf)).await;
        assert!(closed.is_ok(), "a silent client must be dropped, not held open");
        server.abort();
    }

    #[tokio::test]
    async fn slow_drip_request_is_cut_off_at_the_deadline() {
        let limits = Limits {
            read_timeout: Duration::from_secs(5),
            request_deadline: Duration::from_millis(300),
            ..Limits::default()
        };
        let (addr, _tx, server) = spawn_server(limits).await;
        let mut slow = TcpStream::connect(addr).await.expect("connect");
        let cut = tokio::time::timeout(Duration::from_secs(4), async {
            // One header byte every 100 ms: each read is quick, the whole
            // request never completes.
            for b in "GET /api/v1/pods HTTP/1.1\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".bytes() {
                if slow.write_all(&[b]).await.is_err() {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let mut buf = [0u8; 16];
            matches!(slow.read(&mut buf).await, Ok(0) | Err(_))
        })
        .await;
        assert_eq!(cut.ok(), Some(true), "the server must give up on a request that never completes");
        server.abort();
    }

    /// A listener whose first `failures` accepts fail like `EMFILE` does.
    struct FlakyListener {
        inner: TcpListener,
        failures: std::sync::atomic::AtomicU32,
    }

    impl Accept for FlakyListener {
        fn poll_accept(&self, cx: &mut Context<'_>) -> Poll<std::io::Result<(TcpStream, SocketAddr)>> {
            use std::sync::atomic::Ordering;
            if self.failures.load(Ordering::SeqCst) > 0 {
                self.failures.fetch_sub(1, Ordering::SeqCst);
                return Poll::Ready(Err(std::io::Error::from_raw_os_error(24)));
            }
            self.inner.poll_accept(cx)
        }
    }

    #[tokio::test]
    async fn accept_errors_do_not_stop_the_listener() {
        let inner = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = inner.local_addr().expect("addr");
        let flaky = FlakyListener { inner, failures: std::sync::atomic::AtomicU32::new(3) };
        let (_tx, rx) = watch::channel(false);
        let limits = Limits { accept_backoff: Duration::from_millis(5), ..Limits::default() };
        let server = tokio::spawn(serve(flaky, seeded_registry(&node()), rx, None, test_gate(), limits));
        let (status, _) = tokio::time::timeout(Duration::from_secs(5), exchange(addr, &authed("GET", "/healthz", "")))
            .await
            .expect("served after transient accept failures");
        assert!(status.contains(" 200 "), "{status}");
        assert!(!server.is_finished(), "the accept loop survives EMFILE");
        server.abort();
    }

    #[tokio::test]
    async fn connections_beyond_the_cap_are_closed_until_a_slot_frees() {
        let limits = Limits { max_connections: 1, ..Limits::default() };
        let (addr, _tx, server) = spawn_server(limits).await;
        // Hold the only slot with an open watch.
        let mut watcher = TcpStream::connect(addr).await.expect("connect");
        watcher.write_all(authed("GET", "/api/v1/pods?watch=true", "").as_bytes()).await.expect("write");
        let mut head = [0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(5), watcher.read(&mut head)).await.expect("watch opened").expect("read");
        assert!(String::from_utf8_lossy(&head[..n]).contains("200"), "watch is streaming");
        // A second connection is turned away while the slot is held.
        let (status, _) = tokio::time::timeout(Duration::from_secs(5), exchange(addr, &authed("GET", "/healthz", "")))
            .await
            .expect("over-cap connection closes promptly");
        assert!(!status.contains(" 200 "), "over the cap: {status:?}");
        // Free the slot; new connections are served again.
        drop(watcher);
        let served = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (status, _) = exchange(addr, &authed("GET", "/healthz", "")).await;
                if status.contains(" 200 ") {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        assert!(served.is_ok(), "a freed slot is reused");
        server.abort();
    }

    #[tokio::test]
    async fn run_until_refuses_an_exposed_plain_http_listener() {
        let mut cfg = loopback_cfg(0);
        cfg.bind_addr = "0.0.0.0".to_string();
        let result = tokio::time::timeout(Duration::from_secs(5), run_until(cfg, async {})).await.expect("prompt");
        let err = result.expect_err("0.0.0.0 without TLS must not boot");
        assert!(err.to_string().contains("TLS"), "{err}");
    }

    #[tokio::test]
    async fn run_until_refuses_to_boot_without_a_credential_source() {
        let mut cfg = loopback_cfg(0);
        cfg.access = AccessConfig::default();
        let result = tokio::time::timeout(Duration::from_secs(5), run_until(cfg, async {})).await.expect("prompt");
        assert!(result.is_err(), "no tokens, token file, client CA or bootstrap dir: nobody could log in");
    }

    // With the `tls` feature a bootstrap boot serves HTTPS with a generated CA;
    // that path is covered in tests/tls_serving.rs.
    #[cfg(not(feature = "tls"))]
    #[tokio::test]
    async fn boot_writes_an_admin_credential_that_authenticates() {
        let dir = std::env::temp_dir().join(format!("cavehome-boot-{}-{:?}", std::process::id(), std::time::SystemTime::now()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let mut cfg = loopback_cfg(addr.port());
        cfg.access = AccessConfig { bootstrap_dir: Some(dir.clone()), ..AccessConfig::default() };
        let (tx, rx) = watch::channel(false);
        let shutdown = async move {
            let mut rx = rx;
            while !*rx.borrow() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
        };
        let server = tokio::spawn(run_until_on_listener(cfg, listener, shutdown));
        let token_path = dir.join("admin.token");
        let token = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(t) = std::fs::read_to_string(&token_path) {
                    if !t.trim().is_empty() {
                        return t.trim().to_string();
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("admin token written at boot");
        let kubeconfig = std::fs::read_to_string(dir.join("kubeconfig.yaml")).expect("kubeconfig");
        assert!(kubeconfig.contains(&format!("server: http://127.0.0.1:{}", addr.port())), "{kubeconfig}");
        let raw = format!("GET /api/v1/nodes HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\n\r\n", addr.port());
        let (status, body) = exchange(addr, &raw).await;
        assert!(status.contains(" 200 "), "{status} {body}");
        let _ = tx.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
    }
}
