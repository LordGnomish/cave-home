//! Call routing — who rings, in what order, or where the call goes instead.
//!
//! Given an incoming call to an extension (or a ring group), this module
//! computes the [`RouteOutcome`]: ring a planned set of extensions, send the
//! call straight to voicemail, or forward it. It honours, in order:
//!
//! 1. **After-hours schedule** — outside business hours the call does not ring.
//! 2. **Do-not-disturb** — an extension on DND does not ring, unless the call
//!    is flagged an emergency (the DND emergency override).
//! 3. **Call-forwarding** — a forward target re-points the call.
//!
//! For a ring group it applies the per-member checks and then orders the
//! survivors by the group's [`crate::extension::RingStrategy`].

use crate::extension::{CallGroup, Extension, ExtensionNumber, RingStrategy};
use crate::schedule::{BusinessHours, Minute};

/// Where a forwarded call should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardTarget {
    /// Forward to another extension by number.
    Extension(String),
    /// Forward to voicemail explicitly.
    Voicemail,
}

/// Per-extension routing preferences the household configures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingPrefs {
    /// Do-not-disturb: when on, the extension does not ring (unless the call is
    /// an emergency).
    pub do_not_disturb: bool,
    /// An optional call-forward target. Applied when set, after DND.
    pub forward: Option<ForwardTarget>,
}

impl RoutingPrefs {
    /// Convenience: preferences with do-not-disturb on.
    #[must_use]
    pub fn dnd() -> Self {
        Self { do_not_disturb: true, forward: None }
    }

    /// Convenience: preferences forwarding to another extension.
    #[must_use]
    pub fn forwarding_to(number: impl Into<String>) -> Self {
        Self { do_not_disturb: false, forward: Some(ForwardTarget::Extension(number.into())) }
    }
}

/// The ordered set of extensions a call should ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingPlan {
    /// Extension numbers to ring, in priority order. For
    /// [`RingStrategy::RingAll`] they ring simultaneously but the order still
    /// breaks ties; for sequential / round-robin it is the dial order.
    pub order: Vec<ExtensionNumber>,
    /// The strategy the order was produced under (so the dialer knows whether
    /// to ring them at once or one at a time).
    pub strategy: RingStrategy,
}

/// What routing decided to do with an incoming call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOutcome {
    /// Ring this ordered set of extensions.
    Ring(RingPlan),
    /// Nobody is available — send the call to voicemail.
    Voicemail,
    /// Forward the call to another extension number.
    Forward(String),
}

/// Route an incoming call to a single extension.
///
/// `is_emergency` is the do-not-disturb override: an emergency call rings
/// through DND (it does **not** override the after-hours schedule — an empty
/// house is still empty).
#[must_use]
pub fn route_call(
    ext: &Extension,
    prefs: &RoutingPrefs,
    hours: BusinessHours,
    now: Minute,
    is_emergency: bool,
) -> RouteOutcome {
    // 1. After-hours: the schedule wins for non-emergency calls. An emergency
    //    still rings even after hours (someone may be home and want it).
    if hours.is_after_hours(now) && !is_emergency {
        return voicemail_or_forward(ext, prefs);
    }

    // 2. Do-not-disturb, unless this is an emergency override.
    if prefs.do_not_disturb && !is_emergency {
        return voicemail_or_forward(ext, prefs);
    }

    // 3. A forward target re-points the call even when the extension could ring.
    if let Some(ForwardTarget::Extension(target)) = &prefs.forward {
        return RouteOutcome::Forward(target.clone());
    }
    if let Some(ForwardTarget::Voicemail) = &prefs.forward {
        return RouteOutcome::Voicemail;
    }

    // Otherwise: ring the extension.
    RouteOutcome::Ring(RingPlan {
        order: vec![ext.number().clone()],
        strategy: RingStrategy::Sequential,
    })
}

/// What an un-rung single extension falls back to: its forward, else voicemail
/// if it has a box, else voicemail anyway (an empty house always takes a
/// message rather than ringing forever).
fn voicemail_or_forward(ext: &Extension, prefs: &RoutingPrefs) -> RouteOutcome {
    match &prefs.forward {
        Some(ForwardTarget::Extension(target)) => RouteOutcome::Forward(target.clone()),
        Some(ForwardTarget::Voicemail) | None => {
            // With or without an explicit box, the courteous default is
            // voicemail; whether a recording is actually offered is the
            // extension's voicemail flag (consumed by the transport layer).
            let _ = ext.voicemail_enabled();
            RouteOutcome::Voicemail
        }
    }
}

/// Route an incoming call to a ring group.
///
/// Each member is looked up via `lookup` (number → its preferences). Members on
/// DND (without emergency) are dropped from the ring; if everyone is dropped, or
/// it is after hours for a non-emergency call, the group rolls to voicemail.
/// The survivors are ordered by the group's strategy (round-robin uses
/// `rotation`).
#[must_use]
pub fn route_group<'a, F>(
    group: &CallGroup,
    hours: BusinessHours,
    now: Minute,
    is_emergency: bool,
    rotation: usize,
    mut prefs_of: F,
) -> RouteOutcome
where
    F: FnMut(&ExtensionNumber) -> Option<&'a RoutingPrefs>,
{
    if hours.is_after_hours(now) && !is_emergency {
        return RouteOutcome::Voicemail;
    }

    let ordered = group.ring_order(rotation);
    let available: Vec<ExtensionNumber> = ordered
        .into_iter()
        .filter(|num| {
            // Keep the member if they are reachable: not on DND, or an
            // emergency overrides DND. A member with no prefs entry is treated
            // as available (default prefs).
            match prefs_of(num) {
                Some(p) => is_emergency || !p.do_not_disturb,
                None => true,
            }
        })
        .collect();

    if available.is_empty() {
        RouteOutcome::Voicemail
    } else {
        RouteOutcome::Ring(RingPlan { order: available, strategy: group.strategy() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceId;

    fn ext(num: &str) -> Extension {
        Extension::new(num, "Test", DeviceId(1)).unwrap()
    }

    fn num(n: &str) -> ExtensionNumber {
        ext(n).number().clone()
    }

    fn open() -> BusinessHours {
        BusinessHours::always_open()
    }

    fn noon() -> Minute {
        Minute::at(12, 0).unwrap()
    }

    // ---- single-extension routing -----------------------------------------

    #[test]
    fn plain_call_in_hours_rings_the_extension() {
        let e = ext("101");
        let out = route_call(&e, &RoutingPrefs::default(), open(), noon(), false);
        match out {
            RouteOutcome::Ring(plan) => assert_eq!(plan.order, vec![num("101")]),
            other => panic!("expected ring, got {other:?}"),
        }
    }

    #[test]
    fn dnd_sends_to_voicemail() {
        let e = ext("101");
        let out = route_call(&e, &RoutingPrefs::dnd(), open(), noon(), false);
        assert_eq!(out, RouteOutcome::Voicemail);
    }

    #[test]
    fn emergency_overrides_dnd_and_rings() {
        let e = ext("101");
        let out = route_call(&e, &RoutingPrefs::dnd(), open(), noon(), true);
        assert!(matches!(out, RouteOutcome::Ring(_)), "emergency must ring through DND");
    }

    #[test]
    fn forward_repoints_a_ringing_call() {
        let e = ext("101");
        let prefs = RoutingPrefs::forwarding_to("200");
        let out = route_call(&e, &prefs, open(), noon(), false);
        assert_eq!(out, RouteOutcome::Forward("200".to_owned()));
    }

    #[test]
    fn after_hours_non_emergency_goes_to_voicemail() {
        let e = ext("101");
        let hours = BusinessHours::new(Minute::at(8, 0).unwrap(), Minute::at(22, 0).unwrap());
        let late = Minute::at(23, 30).unwrap();
        let out = route_call(&e, &RoutingPrefs::default(), hours, late, false);
        assert_eq!(out, RouteOutcome::Voicemail);
    }

    #[test]
    fn after_hours_forward_uses_the_forward_target() {
        let e = ext("101");
        let hours = BusinessHours::new(Minute::at(8, 0).unwrap(), Minute::at(22, 0).unwrap());
        let late = Minute::at(23, 30).unwrap();
        let prefs = RoutingPrefs::forwarding_to("911");
        let out = route_call(&e, &prefs, hours, late, false);
        assert_eq!(out, RouteOutcome::Forward("911".to_owned()));
    }

    #[test]
    fn after_hours_emergency_still_rings() {
        let e = ext("101");
        let hours = BusinessHours::new(Minute::at(8, 0).unwrap(), Minute::at(22, 0).unwrap());
        let late = Minute::at(23, 30).unwrap();
        let out = route_call(&e, &RoutingPrefs::default(), hours, late, true);
        assert!(matches!(out, RouteOutcome::Ring(_)), "an emergency rings even after hours");
    }

    #[test]
    fn explicit_voicemail_forward_target() {
        let e = ext("101");
        let prefs = RoutingPrefs { do_not_disturb: false, forward: Some(ForwardTarget::Voicemail) };
        let out = route_call(&e, &prefs, open(), noon(), false);
        assert_eq!(out, RouteOutcome::Voicemail);
    }

    // ---- ring-group routing ------------------------------------------------

    #[test]
    fn ring_all_group_rings_every_available_member() {
        let g = CallGroup::new(
            "House",
            RingStrategy::RingAll,
            vec![num("101"), num("102"), num("103")],
        )
        .unwrap();
        let out = route_group(&g, open(), noon(), false, 0, |_| None);
        match out {
            RouteOutcome::Ring(plan) => {
                assert_eq!(plan.strategy, RingStrategy::RingAll);
                assert_eq!(plan.order, vec![num("101"), num("102"), num("103")]);
            }
            other => panic!("expected ring, got {other:?}"),
        }
    }

    #[test]
    fn group_drops_members_on_dnd() {
        let g = CallGroup::new(
            "House",
            RingStrategy::Sequential,
            vec![num("101"), num("102"), num("103")],
        )
        .unwrap();
        let dnd = RoutingPrefs::dnd();
        let out = route_group(&g, open(), noon(), false, 0, |n| {
            if n.as_str() == "102" { Some(&dnd) } else { None }
        });
        match out {
            RouteOutcome::Ring(plan) => assert_eq!(plan.order, vec![num("101"), num("103")]),
            other => panic!("expected ring, got {other:?}"),
        }
    }

    #[test]
    fn group_all_on_dnd_rolls_to_voicemail() {
        let g = CallGroup::new("House", RingStrategy::RingAll, vec![num("101"), num("102")])
            .unwrap();
        let dnd = RoutingPrefs::dnd();
        let out = route_group(&g, open(), noon(), false, 0, |_| Some(&dnd));
        assert_eq!(out, RouteOutcome::Voicemail);
    }

    #[test]
    fn group_emergency_overrides_member_dnd() {
        let g = CallGroup::new("House", RingStrategy::RingAll, vec![num("101"), num("102")])
            .unwrap();
        let dnd = RoutingPrefs::dnd();
        let out = route_group(&g, open(), noon(), true, 0, |_| Some(&dnd));
        match out {
            RouteOutcome::Ring(plan) => assert_eq!(plan.order.len(), 2),
            other => panic!("emergency should ring everyone, got {other:?}"),
        }
    }

    #[test]
    fn round_robin_group_rotates_then_filters() {
        let g = CallGroup::new(
            "Support",
            RingStrategy::RoundRobin,
            vec![num("101"), num("102"), num("103")],
        )
        .unwrap();
        // rotation 1 starts at 102; nobody on DND.
        let out = route_group(&g, open(), noon(), false, 1, |_| None);
        match out {
            RouteOutcome::Ring(plan) => {
                assert_eq!(plan.strategy, RingStrategy::RoundRobin);
                assert_eq!(plan.order, vec![num("102"), num("103"), num("101")]);
            }
            other => panic!("expected ring, got {other:?}"),
        }
    }

    #[test]
    fn group_after_hours_non_emergency_voicemail() {
        let g = CallGroup::new("House", RingStrategy::RingAll, vec![num("101")]).unwrap();
        let hours = BusinessHours::new(Minute::at(8, 0).unwrap(), Minute::at(22, 0).unwrap());
        let late = Minute::at(2, 0).unwrap();
        let out = route_group(&g, hours, late, false, 0, |_| None);
        assert_eq!(out, RouteOutcome::Voicemail);
    }
}
