//! UI language selector for grandma-friendly wellness copy.
//!
//! Charter §6.3 mandates EN + DE + TR from M1. Wellness copy is deliberately
//! gentle and **non-clinical** (ADR-025, Charter §6.3): the engine reports
//! "you slept well", never a diagnosis. See [`crate::band`] for the localized
//! names and advice that consume this.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}
