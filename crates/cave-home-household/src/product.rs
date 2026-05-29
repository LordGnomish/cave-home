//! The product model — the inventory entry every other module reasons about.
//!
//! A [`Product`] is vendor-neutral and storage-neutral: a barcode lookup, a
//! product-database import or a persistent store (all deferred to Phase 1b, see
//! the parity manifest) build these values, and everything downstream — stock
//! operations, the shopping list, expiry tracking, recipe checks — works off
//! this model alone. There is no clock: the caller supplies "today" as an
//! integer day number, so the whole crate is pure and deterministic.

use crate::label::Lang;

/// How a product's amount is measured. Kept deliberately small for the MVP;
/// what matters downstream is the [`QuantityUnit::purchase_step`] used to round
/// a shortfall up to whole shop-able units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QuantityUnit {
    /// Counted whole items (eggs, cans, bottles). Purchased one at a time.
    Piece,
    /// A pack of a fixed count (a six-pack, a box of ten). Purchased per pack.
    Pack(u32),
    /// A continuous weight in grams. Purchased in `step` grams (e.g. a 500 g bag).
    Grams { step: f64 },
    /// A continuous volume in millilitres. Purchased in `step` ml (e.g. a 1 L carton).
    Millilitres { step: f64 },
}

impl QuantityUnit {
    /// The smallest amount that can actually be bought at the shop. A shortfall
    /// is always rounded *up* to a whole multiple of this so the household
    /// never comes home one egg short.
    #[must_use]
    pub fn purchase_step(self) -> f64 {
        match self {
            Self::Piece => 1.0,
            Self::Pack(n) => f64::from(n.max(1)),
            Self::Grams { step } | Self::Millilitres { step } => {
                if step > 0.0 {
                    step
                } else {
                    1.0
                }
            }
        }
    }

    /// The household-facing unit word (no jargon — Charter §6.3).
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Piece | Self::Pack(_), Lang::En) => "pcs",
            (Self::Piece | Self::Pack(_), Lang::De) => "Stk.",
            (Self::Piece | Self::Pack(_), Lang::Tr) => "adet",
            (Self::Grams { .. }, _) => "g",
            (Self::Millilitres { .. }, _) => "ml",
        }
    }
}

/// Why a [`Product`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductError {
    /// The name was empty or whitespace only.
    EmptyName,
    /// A supplied amount was `NaN`, infinite or negative.
    BadAmount,
}

impl core::fmt::Display for ProductError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyName => f.write_str("a product needs a name"),
            Self::BadAmount => f.write_str("an amount must be a finite, non-negative number"),
        }
    }
}

impl std::error::Error for ProductError {}

/// One thing the household keeps in stock: food, a medicine, a battery type.
///
/// Construction validates the name and all amounts up front so no module
/// downstream has to defend against `NaN`, negative stock or an empty label.
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    name: String,
    unit: QuantityUnit,
    stock: f64,
    min_stock: f64,
    /// Day number (caller's "today" scale) this product is best before, if any.
    best_before: Option<i64>,
}

impl Product {
    /// Construct a validated product.
    ///
    /// `stock` and `min_stock` are clamped-checked (finite, non-negative);
    /// `best_before` is an optional day number on the caller's "today" scale.
    ///
    /// # Errors
    /// Returns [`ProductError::EmptyName`] for a blank name, or
    /// [`ProductError::BadAmount`] for a non-finite/negative amount.
    pub fn new(
        name: impl Into<String>,
        unit: QuantityUnit,
        stock: f64,
        min_stock: f64,
        best_before: Option<i64>,
    ) -> Result<Self, ProductError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ProductError::EmptyName);
        }
        if !Self::ok_amount(stock) || !Self::ok_amount(min_stock) {
            return Err(ProductError::BadAmount);
        }
        Ok(Self {
            name,
            unit,
            stock,
            min_stock,
            best_before,
        })
    }

    fn ok_amount(v: f64) -> bool {
        v.is_finite() && v >= 0.0
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn unit(&self) -> QuantityUnit {
        self.unit
    }

    #[must_use]
    pub const fn stock(&self) -> f64 {
        self.stock
    }

    #[must_use]
    pub const fn min_stock(&self) -> f64 {
        self.min_stock
    }

    #[must_use]
    pub const fn best_before(&self) -> Option<i64> {
        self.best_before
    }

    /// Set the on-hand stock to a validated amount. Used by [`crate::stock`].
    ///
    /// # Errors
    /// Returns [`ProductError::BadAmount`] if `amount` is non-finite or negative.
    pub fn set_stock(&mut self, amount: f64) -> Result<(), ProductError> {
        if Self::ok_amount(amount) {
            self.stock = amount;
            Ok(())
        } else {
            Err(ProductError::BadAmount)
        }
    }

    /// Is this product at or below its minimum stock? (The shopping-list trigger.)
    #[must_use]
    pub fn is_below_min(&self) -> bool {
        self.stock < self.min_stock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_name() {
        assert_eq!(
            Product::new("   ", QuantityUnit::Piece, 1.0, 0.0, None),
            Err(ProductError::EmptyName)
        );
    }

    #[test]
    fn rejects_bad_amounts() {
        assert_eq!(
            Product::new("Milk", QuantityUnit::Piece, f64::NAN, 0.0, None),
            Err(ProductError::BadAmount)
        );
        assert_eq!(
            Product::new("Milk", QuantityUnit::Piece, -1.0, 0.0, None),
            Err(ProductError::BadAmount)
        );
    }

    #[test]
    fn purchase_step_rounds_to_shopable_units() {
        assert_eq!(QuantityUnit::Piece.purchase_step(), 1.0);
        assert_eq!(QuantityUnit::Pack(6).purchase_step(), 6.0);
        assert_eq!(QuantityUnit::Grams { step: 500.0 }.purchase_step(), 500.0);
        // A degenerate zero/negative step never divides by zero downstream.
        assert_eq!(QuantityUnit::Grams { step: 0.0 }.purchase_step(), 1.0);
        assert_eq!(QuantityUnit::Pack(0).purchase_step(), 1.0);
    }

    #[test]
    fn below_min_is_strict() {
        let exactly_at = Product::new("Eggs", QuantityUnit::Piece, 6.0, 6.0, None).unwrap();
        assert!(!exactly_at.is_below_min(), "at the minimum is not below it");
        let under = Product::new("Eggs", QuantityUnit::Piece, 5.0, 6.0, None).unwrap();
        assert!(under.is_below_min());
    }
}
