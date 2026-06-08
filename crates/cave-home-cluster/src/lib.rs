//! `cave-home-cluster` — the multi-node topology + active-passive failover
//! **decision engine** for a cave-home deployment (Charter §5, ADR-005).
//!
//! A cave-home home runs as a small bare-metal cluster: a **primary hub**, an
//! optional **backup hub** (active-passive failover for the primary), and an
//! optional **ML / GPU node** that off-loads camera inference. This crate is
//! the pure-logic brain that answers the cluster-lifecycle questions:
//!
//! - Is the cluster valid? (exactly one active primary; at least one node that
//!   *can* be primary) — [`topology`].
//! - From a node's last heartbeat tick and the current tick, is that node
//!   healthy, degraded, or out of touch? — [`health`].
//! - The primary went out of touch and a backup is healthy — should we promote
//!   the backup, and may we (split-brain / fencing guard)? When the old primary
//!   comes back, do we fail back automatically or wait for a human? —
//!   [`failover`].
//! - Which node should run a given workload (radios / camera inference /
//!   automation)? — [`placement`].
//! - With 1–3 nodes, is the cluster operational, running on its last legs, or
//!   down? — [`quorum`].
//! - During an update, which node is safe to take down and in what order? —
//!   [`update`].
//! - Plain-language EN / DE / TR status lines for the homeowner — [`label`].
//!
//! # Scope (Phase 1 MVP)
//!
//! This crate is **pure logic, std-only**: no network, no clock, no consensus
//! transport. The caller supplies the current tick (a monotonic counter) and
//! the observed heartbeats; this crate makes the *decisions*. The actual
//! heartbeat/gossip transport, the leader-election wire protocol (Raft / lease),
//! real STONITH-class fencing, and the wiring into the ADR-004 K3s orchestration
//! layer + `cave-home-node-discovery` are network/consensus-bound and are
//! deferred to Phase 1b — every one is enumerated in `parity.manifest.toml`
//! `[[unmapped]]` with an ADR-004 / ADR-005 disposition. The decision the
//! protocol *implements* lives here; only the wire does not.
//!
//! # Example
//!
//! ```
//! use cave_home_cluster::{
//!     Cluster, Node, NodeRole, NodeHealth, Lang, FailoverPlan, FenceStatus,
//! };
//!
//! // A primary hub plus a backup hub, observed at tick 1000.
//! let mut cluster = Cluster::new();
//! cluster.add(Node::new("hub-1", "Living-room hub", NodeRole::Primary).with_radios());
//! cluster.add(Node::new("hub-2", "Closet backup", NodeRole::BackupHub));
//! assert!(cluster.validate().is_ok());
//!
//! // The primary has not been heard from for a long time; the backup is fresh.
//! cluster.set_heartbeat("hub-1", 100);
//! cluster.set_heartbeat("hub-2", 995);
//!
//! // Fencing confirms the old primary really is down -> promote the backup.
//! let plan = cluster.decide_failover(1000, FenceStatus::Confirmed);
//! assert!(matches!(plan, FailoverPlan::Promote { ref node, .. } if node == "hub-2"));
//!
//! // The homeowner just sees: "Backup hub took over — everything still works."
//! println!("{}", plan.headline(Lang::En));
//! ```

pub mod failover;
pub mod health;
pub mod label;
pub mod node;
pub mod placement;
pub mod quorum;
pub mod topology;
pub mod update;

pub use failover::{FailbackPolicy, FailoverPlan, FenceStatus};
pub use health::{HealthThresholds, classify_health};
pub use label::Lang;
pub use node::{Capabilities, Node, NodeHealth, NodeRole};
pub use placement::{PlacementError, Workload, place};
pub use quorum::ClusterStatus;
pub use topology::{Cluster, TopologyError};
pub use update::{DrainError, DrainPlan};
