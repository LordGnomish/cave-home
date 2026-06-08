//! The node model — a single machine in a cave-home cluster.
//!
//! A node carries an identity, a *role* (what it is for), a *health* (how it is
//! doing right now), the tick of its last heartbeat, what hardware it has, and
//! its software version. Everything else in the crate reasons over collections
//! of these.

/// What a node is for in the cluster (Charter §5 deployment topology).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeRole {
    /// The primary hub: broker, radios, automation engine, Portal. Exactly one
    /// node is the *active* primary at a time (the topology invariant).
    Primary,
    /// A backup hub on standby — active-passive failover for the primary. It can
    /// be promoted to primary when the primary goes out of touch.
    BackupHub,
    /// An ML / GPU node that off-loads camera inference. Never serves as the
    /// primary hub even if it is the only other node up.
    MlGpu,
}

impl NodeRole {
    /// Whether a node in this role can ever *become* the active primary hub.
    /// The primary itself and a backup hub can; an ML/GPU node cannot.
    #[must_use]
    pub const fn is_primary_capable(self) -> bool {
        matches!(self, Self::Primary | Self::BackupHub)
    }
}

/// How a node is doing right now, derived from heartbeat age (see
/// [`crate::health`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeHealth {
    /// Heartbeats are arriving on time.
    Healthy,
    /// Heartbeats are late — the node is reachable but should not be trusted
    /// with a failover decision until it recovers.
    Degraded,
    /// No heartbeat within the unreachable window — treat as down.
    Unreachable,
}

/// The hardware a node has, which gates workload placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// Has the Zigbee / Matter / Z-Wave radios attached.
    pub has_radios: bool,
    /// Has a usable inference accelerator (Coral / NVIDIA / `OpenVINO` class).
    pub has_gpu: bool,
}

/// One machine in the cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    id: String,
    name: String,
    role: NodeRole,
    health: NodeHealth,
    /// The tick of the last heartbeat received from this node. `None` until the
    /// node has ever been heard from.
    last_heartbeat: Option<u64>,
    capabilities: Capabilities,
    version: String,
    /// Whether the node has been marked for a controlled take-down (rolling
    /// update). A draining node accepts no new workloads.
    draining: bool,
}

impl Node {
    /// Construct a node with a stable id, a friendly name, and a role.
    ///
    /// The node starts [`NodeHealth::Unreachable`] with no heartbeat — it is not
    /// trusted until a heartbeat is recorded and classified. Capabilities start
    /// empty; version starts empty.
    #[must_use]
    pub fn new(id: &str, name: &str, role: NodeRole) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            role,
            health: NodeHealth::Unreachable,
            last_heartbeat: None,
            capabilities: Capabilities::default(),
            version: String::new(),
            draining: false,
        }
    }

    /// Builder: mark this node as having the radios attached.
    #[must_use]
    pub const fn with_radios(mut self) -> Self {
        self.capabilities.has_radios = true;
        self
    }

    /// Builder: mark this node as having an inference accelerator.
    #[must_use]
    pub const fn with_gpu(mut self) -> Self {
        self.capabilities.has_gpu = true;
        self
    }

    /// Builder: record the node's software version.
    #[must_use]
    pub fn with_version(mut self, version: &str) -> Self {
        version.clone_into(&mut self.version);
        self
    }

    // Accessors return borrows of `String` fields as `&str`. These are not
    // marked `const fn`: the `&str` view of a `String` is only `const` from Rust
    // 1.87, below this workspace's effective MSRV.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn role(&self) -> NodeRole {
        self.role
    }

    #[must_use]
    pub const fn health(&self) -> NodeHealth {
        self.health
    }

    #[must_use]
    pub const fn last_heartbeat(&self) -> Option<u64> {
        self.last_heartbeat
    }

    #[must_use]
    pub const fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub const fn is_draining(&self) -> bool {
        self.draining
    }

    /// Record a heartbeat at `tick`. Health is *not* recomputed here — that is
    /// the caller's job via [`Node::refresh_health`] with the current tick and
    /// thresholds, because "how late is too late" is a policy decision.
    pub const fn set_heartbeat(&mut self, tick: u64) {
        self.last_heartbeat = Some(tick);
    }

    /// Recompute and store this node's health from its heartbeat age.
    pub const fn refresh_health(&mut self, now: u64, thresholds: crate::health::HealthThresholds) {
        self.health = crate::health::classify_health(self.last_heartbeat, now, thresholds);
    }

    /// Force a role change (used by failover promotion / demotion).
    pub const fn set_role(&mut self, role: NodeRole) {
        self.role = role;
    }

    /// Mark this node draining (true) or clear the mark (false).
    pub const fn set_draining(&mut self, draining: bool) {
        self.draining = draining;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthThresholds;

    #[test]
    fn primary_capability_matches_role() {
        assert!(NodeRole::Primary.is_primary_capable());
        assert!(NodeRole::BackupHub.is_primary_capable());
        assert!(!NodeRole::MlGpu.is_primary_capable());
    }

    #[test]
    fn new_node_starts_unreachable_with_no_heartbeat() {
        let n = Node::new("a", "Hub", NodeRole::Primary);
        assert_eq!(n.health(), NodeHealth::Unreachable);
        assert_eq!(n.last_heartbeat(), None);
        assert!(!n.is_draining());
        assert_eq!(n.version(), "");
    }

    #[test]
    fn builders_set_capabilities_and_version() {
        let n = Node::new("a", "Hub", NodeRole::Primary)
            .with_radios()
            .with_gpu()
            .with_version("0.1.0");
        assert!(n.capabilities().has_radios);
        assert!(n.capabilities().has_gpu);
        assert_eq!(n.version(), "0.1.0");
    }

    #[test]
    fn refresh_health_reflects_recorded_heartbeat() {
        let t = HealthThresholds::default();
        let mut n = Node::new("a", "Hub", NodeRole::Primary);
        n.set_heartbeat(100);
        n.refresh_health(100, t);
        assert_eq!(n.health(), NodeHealth::Healthy);
    }

    #[test]
    fn health_ordering_is_best_to_worst() {
        assert!(NodeHealth::Healthy < NodeHealth::Degraded);
        assert!(NodeHealth::Degraded < NodeHealth::Unreachable);
    }
}
