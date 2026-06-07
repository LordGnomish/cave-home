// SPDX-License-Identifier: Apache-2.0
//! `cave-home-binary` — the unified single-binary bootstrap/config/dispatch core.
//!
//! Per Charter §5, every cave-home stack on a node compiles into **one** Rust
//! binary: it is the unit of install, the unit of upgrade, and the unit of
//! rollback. There are no sub-processes, sidecars, or helper daemons — the hub
//! *and* the orchestration layer live in the same process.
//!
//! This crate is the **pure logic** of that binary, the part that decides
//! *what* the process should do before any of it actually happens:
//!
//! - [`config`] — the layered node configuration model
//!   (defaults < config file < environment < command-line flags) with
//!   deterministic, documented precedence and validation.
//! - [`cli`] — hand-rolled argument parsing into a typed [`cli::Command`],
//!   with grandma-friendly help text (Charter §6.3, ADR-007).
//! - [`bootstrap`] — given a validated config, the ordered bring-up plan: which
//!   pillars and the orchestration layer to start, and in what order. Planning
//!   only — nothing is launched here.
//! - [`shutdown`] — the graceful-shutdown plan (the bring-up order, reversed).
//! - [`health`] — aggregation of per-component states into the binary's overall
//!   up / degraded / down readiness verdict.
//! - [`version`] — honest build/version info (version, git sha *iff* the build
//!   environment supplied one, build profile, supported-pillar list).
//!
//! # What is deferred (Phase 1b)
//!
//! The async runtime, the actual process launch, OS signal handling, the *real*
//! component start sequence, and the install / upgrade / rollback mechanics are
//! deferred and enumerated in `parity.manifest.toml` `[[unmapped]]`. This crate
//! computes the plans those phases will execute; it deliberately performs no
//! I/O so the decisions are fully unit-testable.
//!
//! # Single-binary invariant
//!
//! Every [`Component`] this crate knows about runs **in-process** in the one
//! binary. [`bootstrap::Plan::is_single_binary`] asserts that invariant over a
//! computed plan; there is no concept here of spawning a separate OS process
//! for a pillar.

pub mod bootstrap;
pub mod cli;
pub mod config;
pub mod health;
pub mod http;
pub mod shutdown;
pub mod version;

pub use bootstrap::{Plan, PlanError};
pub use cli::{Command, ParseError};
pub use config::{Config, ConfigError, Layer, NodeRole};
pub use health::{ComponentHealth, Health, HealthState};
pub use version::BuildInfo;

/// A first-class cave-home subsystem that runs **in-process** in the single
/// binary (Charter §5). This is the unit the bootstrap/shutdown plans order.
///
/// Modelled as a closed enum on purpose: this pure-logic crate names the
/// component identities rather than depending on every sibling crate, so the
/// planner stays std-only and fully testable. The real wiring that maps a
/// [`Component`] to its owning crate's start function is the deferred Phase 1b
/// work (see `parity.manifest.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Component {
    /// The native K3s-class orchestration layer (ADR-004). It hosts every other
    /// in-process pillar, so it comes up first and goes down last.
    Orchestration,
    /// The embedded Mosquitto-class message broker. Pillars publish/subscribe
    /// through it, so it precedes them.
    Broker,
    /// The HA-core port: state machine + event bus + automation engine
    /// (`cave-home-core` / `cave-home-automation`) — the behavioural heart.
    Core,
    /// Device protocol stacks (`Zigbee` / `Matter` / `Z-Wave` / `ESPHome` / `MQTT`).
    Integrations,
    /// Camera / NVR (Frigate-class) ingest + inference.
    Cameras,
    /// Local voice assistant (STT + TTS + wake word + intent routing).
    Voice,
    /// The built-in Portal dashboard + API the household actually talks to.
    Portal,
}

impl Component {
    /// All components this binary understands, in a stable canonical order.
    pub const ALL: [Self; 7] = [
        Self::Orchestration,
        Self::Broker,
        Self::Core,
        Self::Integrations,
        Self::Cameras,
        Self::Voice,
        Self::Portal,
    ];

    /// The stable lowercase identifier used in config files, flags and plans.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Orchestration => "orchestration",
            Self::Broker => "broker",
            Self::Core => "core",
            Self::Integrations => "integrations",
            Self::Cameras => "cameras",
            Self::Voice => "voice",
            Self::Portal => "portal",
        }
    }

    /// A grandma-friendly, jargon-free label for the household-facing surfaces
    /// (Charter §6.3). No implementation vocabulary (no "broker", "orchestration",
    /// "MQTT") leaks here.
    #[must_use]
    pub const fn friendly_name(self) -> &'static str {
        match self {
            Self::Orchestration => "Home foundation",
            Self::Broker => "Device messaging",
            Self::Core => "Automations and rules",
            Self::Integrations => "Connected devices",
            Self::Cameras => "Cameras",
            Self::Voice => "Voice control",
            Self::Portal => "Home dashboard",
        }
    }

    /// Parse a component from its config/flag [`key`](Self::key).
    #[must_use]
    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_keys_roundtrip() {
        for c in Component::ALL {
            assert_eq!(Component::from_key(c.key()), Some(c));
        }
    }

    #[test]
    fn unknown_component_key_is_none() {
        assert_eq!(Component::from_key("nope"), None);
        assert_eq!(Component::from_key(""), None);
    }

    #[test]
    fn component_keys_are_unique() {
        let mut keys: Vec<&str> = Component::ALL.iter().map(|c| c.key()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "component keys must be unique");
    }

    #[test]
    fn friendly_names_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-007: household-facing labels stay in home vocabulary.
        let banned = [
            "pod", "kubelet", "etcd", "namespace", "rbac", "mqtt", "broker",
            "orchestration", "k3s", "container", "zigbee", "modbus",
        ];
        for c in Component::ALL {
            let lower = c.friendly_name().to_ascii_lowercase();
            for b in banned {
                assert!(
                    !lower.contains(b),
                    "component {c:?} friendly_name leaks jargon: {b}"
                );
            }
        }
    }
}
