//! Grandma-friendly localisation for the household engine (Charter §6.3,
//! ADR-007, ADR-026).
//!
//! The end-user never sees a stock-entry ID, a "Grocy API" reference, a
//! quantity-unit foreign key or the word "SKU". The Portal and the mobile app
//! show a plain-language line in the household's language — EN / DE / TR, the
//! Charter §6.3 mandatory set from M1. Every user-facing string this crate
//! produces is funnelled through this module's types so it can be checked for
//! jargon in one place (see the crate-level test).

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl Lang {
    /// The three languages cave-home must support from M1, in display order.
    pub const ALL: [Self; 3] = [Self::En, Self::De, Self::Tr];
}
