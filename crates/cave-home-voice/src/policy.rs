//! Family-aware authorization for voice commands (Charter §6.3, ADR-007).
//!
//! Knowing *what* was said (the [`crate::route::IntentAction`]) is not enough:
//! the house also needs to know *who* is allowed to do it. A guest should be
//! able to turn the lights on but not open the garage; a child can ask the
//! temperature but a grown-up confirms before the heating changes.
//!
//! This module is the pure-logic decision core: given an action and the
//! speaker's [`PermissionLevel`], it returns a [`Decision`] (allow / ask for
//! confirmation / refuse). It deliberately does **not** identify the speaker —
//! mapping a voice to a household member is the per-user-profile layer, which is
//! audio/ML-bound and deferred (see `parity.manifest.toml`). This core takes the
//! level as an input so it can be tested and reused independently.

use crate::route::IntentAction;

/// A household member's permission level, most-privileged first.
///
/// The speaker-identification layer (per-user voice profile, deferred) produces
/// this; the policy here consumes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// The home owner / a resident adult — full control.
    Admin,
    /// A trusted family member — everyday control; security needs confirming.
    Member,
    /// A child — comfort and information, but not climate or access changes.
    Child,
    /// A visitor — lights, scenes and questions only.
    Guest,
}

/// How risky an action is, independent of who asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// Harmless and easily reversed: lights, brightness, scenes, questions.
    Routine,
    /// Affects comfort/energy for everyone: heating and cooling.
    Sensitive,
    /// Physical access or safety: covers, including the garage door.
    Restricted,
}

/// What the house should do with a command once it knows who asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Carry it out.
    Allow,
    /// Carry it out only after an explicit confirmation.
    Confirm,
    /// Refuse it.
    Deny,
}

impl Decision {
    /// Whether the command may proceed without any further checks.
    #[must_use]
    pub fn is_allowed(self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// Classify an action by how risky it is to perform.
#[must_use]
pub fn sensitivity(action: &IntentAction) -> Sensitivity {
    match action {
        IntentAction::SetLight { .. }
        | IntentAction::SetBrightness { .. }
        | IntentAction::ActivateScene { .. }
        | IntentAction::QueryState { .. } => Sensitivity::Routine,
        IntentAction::SetTemperature { .. } => Sensitivity::Sensitive,
        IntentAction::SetCover { .. } => Sensitivity::Restricted,
    }
}

/// Decide whether `level` may perform `action`, using the default family policy.
///
/// The policy is the sensible grandma-friendly default; a future Phase-2 layer
/// can make it configurable per household. The table is:
///
/// | level \ sensitivity | Routine | Sensitive | Restricted |
/// |---------------------|---------|-----------|------------|
/// | Admin               | Allow   | Allow     | Allow      |
/// | Member              | Allow   | Allow     | Confirm    |
/// | Child               | Allow   | Confirm   | Deny       |
/// | Guest               | Allow   | Deny      | Deny       |
#[must_use]
pub fn authorize(action: &IntentAction, level: PermissionLevel) -> Decision {
    use Decision::{Allow, Confirm, Deny};
    use PermissionLevel::{Admin, Child, Guest, Member};
    use Sensitivity::{Restricted, Routine, Sensitive};

    match (sensitivity(action), level) {
        (Routine, _) | (Sensitive, Admin | Member) | (Restricted, Admin) => Allow,
        (Sensitive, Child) | (Restricted, Member) => Confirm,
        (Sensitive, Guest) | (Restricted, Child | Guest) => Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::{IntentAction, QueryKind};

    fn light() -> IntentAction {
        IntentAction::SetLight {
            target: "kitchen".into(),
            on: true,
        }
    }
    fn brightness() -> IntentAction {
        IntentAction::SetBrightness {
            target: "kitchen".into(),
            percent: 50,
        }
    }
    fn scene() -> IntentAction {
        IntentAction::ActivateScene {
            name: "movie night".into(),
        }
    }
    fn query() -> IntentAction {
        IntentAction::QueryState {
            target: "bedroom".into(),
            what: QueryKind::Temperature,
        }
    }
    fn climate() -> IntentAction {
        IntentAction::SetTemperature {
            target: "living room".into(),
            celsius: 21,
        }
    }
    fn cover() -> IntentAction {
        IntentAction::SetCover {
            target: "garage".into(),
            open: true,
        }
    }

    const LEVELS: [PermissionLevel; 4] = [
        PermissionLevel::Admin,
        PermissionLevel::Member,
        PermissionLevel::Child,
        PermissionLevel::Guest,
    ];

    #[test]
    fn lights_scenes_and_queries_are_routine() {
        assert_eq!(sensitivity(&light()), Sensitivity::Routine);
        assert_eq!(sensitivity(&brightness()), Sensitivity::Routine);
        assert_eq!(sensitivity(&scene()), Sensitivity::Routine);
        assert_eq!(sensitivity(&query()), Sensitivity::Routine);
    }

    #[test]
    fn climate_is_sensitive() {
        assert_eq!(sensitivity(&climate()), Sensitivity::Sensitive);
    }

    #[test]
    fn covers_are_restricted() {
        // Covers include the garage door — physical access, so restricted.
        assert_eq!(sensitivity(&cover()), Sensitivity::Restricted);
    }

    #[test]
    fn everyone_may_do_routine_things() {
        for level in LEVELS {
            assert_eq!(authorize(&light(), level), Decision::Allow);
            assert_eq!(authorize(&scene(), level), Decision::Allow);
            assert_eq!(authorize(&query(), level), Decision::Allow);
        }
    }

    #[test]
    fn admin_may_do_everything_outright() {
        assert_eq!(authorize(&climate(), PermissionLevel::Admin), Decision::Allow);
        assert_eq!(authorize(&cover(), PermissionLevel::Admin), Decision::Allow);
    }

    #[test]
    fn members_change_climate_but_confirm_covers() {
        assert_eq!(authorize(&climate(), PermissionLevel::Member), Decision::Allow);
        assert_eq!(authorize(&cover(), PermissionLevel::Member), Decision::Confirm);
    }

    #[test]
    fn children_confirm_climate_and_may_not_open_covers() {
        assert_eq!(authorize(&climate(), PermissionLevel::Child), Decision::Confirm);
        assert_eq!(authorize(&cover(), PermissionLevel::Child), Decision::Deny);
    }

    #[test]
    fn guests_may_not_touch_climate_or_covers() {
        assert_eq!(authorize(&climate(), PermissionLevel::Guest), Decision::Deny);
        assert_eq!(authorize(&cover(), PermissionLevel::Guest), Decision::Deny);
    }

    #[test]
    fn decision_allow_helper() {
        assert!(Decision::Allow.is_allowed());
        assert!(!Decision::Confirm.is_allowed());
        assert!(!Decision::Deny.is_allowed());
    }
}
