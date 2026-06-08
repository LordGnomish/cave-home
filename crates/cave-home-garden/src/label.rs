//! Grandma-friendly localisation for the garden engine (Charter §6.3, ADR-029).
//!
//! The end-user never sees a soil-moisture percentage, a lux figure, or the
//! word "sensor". The Portal and the mobile app show a plain-language line in
//! the household's language — EN / DE / TR, the Charter §6.3 mandatory set from
//! M1. Every user-facing string in this crate is produced through this `Lang`
//! so it can be checked for jargon in one place (see the crate-level test).

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}
