//! Stock operations — the pure functions that move inventory in and out.
//!
//! Every operation takes a [`Product`] and returns a *new* validated stock
//! amount (or an error); none mutates global state, none touches a clock or a
//! store. "Open a package" is tracked as a distinct event from "consume" so a
//! Phase 1b store can record opened-package state, but in the MVP it is a
//! no-op-on-stock that simply confirms there is something to open.

use crate::product::{Product, ProductError};

/// Why a stock operation could not be applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StockError {
    /// Tried to consume/open more than is in stock.
    NotEnoughStock {
        /// What the household tried to take.
        wanted: f64,
        /// What was actually on hand.
        available: f64,
    },
    /// The amount itself was nonsense (`NaN`, infinite or negative).
    BadAmount,
}

impl core::fmt::Display for StockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotEnoughStock { wanted, available } => write!(
                f,
                "tried to use {wanted} but only {available} is in stock"
            ),
            Self::BadAmount => f.write_str("the amount must be a finite, non-negative number"),
        }
    }
}

impl std::error::Error for StockError {}

impl From<ProductError> for StockError {
    fn from(_: ProductError) -> Self {
        Self::BadAmount
    }
}

fn checked_amount(amount: f64) -> Result<f64, StockError> {
    if amount.is_finite() && amount >= 0.0 {
        Ok(amount)
    } else {
        Err(StockError::BadAmount)
    }
}

/// Take `amount` out of stock (someone used it).
///
/// Returns the new on-hand amount. Cannot go below zero — taking more than is
/// available is a [`StockError::NotEnoughStock`], never a silent underflow.
///
/// # Errors
/// [`StockError::BadAmount`] for a bad amount, [`StockError::NotEnoughStock`]
/// when `amount` exceeds the current stock.
pub fn consume(product: &Product, amount: f64) -> Result<f64, StockError> {
    let amount = checked_amount(amount)?;
    let available = product.stock();
    if amount > available {
        return Err(StockError::NotEnoughStock { wanted: amount, available });
    }
    Ok(available - amount)
}

/// Add `amount` to stock (the household bought / restocked it).
///
/// Returns the new on-hand amount.
///
/// # Errors
/// [`StockError::BadAmount`] for a non-finite/negative amount.
pub fn purchase(product: &Product, amount: f64) -> Result<f64, StockError> {
    let amount = checked_amount(amount)?;
    Ok(product.stock() + amount)
}

/// Open one package of `unit_size` (e.g. a 1 L carton of milk).
///
/// Opening does not change the amount in stock — the carton is still there,
/// just open — but it confirms there is at least `unit_size` to open. Returns
/// the unchanged stock so a caller can chain it like the other operations and a
/// Phase 1b store can hang opened-package bookkeeping off the same call.
///
/// # Errors
/// [`StockError::BadAmount`] for a bad size, [`StockError::NotEnoughStock`] if
/// there is not a full package to open.
pub fn open(product: &Product, unit_size: f64) -> Result<f64, StockError> {
    let unit_size = checked_amount(unit_size)?;
    let available = product.stock();
    if unit_size > available {
        return Err(StockError::NotEnoughStock { wanted: unit_size, available });
    }
    Ok(available)
}

/// Apply a new stock amount back onto a product in place, validating it.
///
/// A convenience for callers that compute a new amount with [`consume`] /
/// [`purchase`] and want to commit it.
///
/// # Errors
/// [`StockError::BadAmount`] if `new_stock` is non-finite or negative.
pub fn apply(product: &mut Product, new_stock: f64) -> Result<(), StockError> {
    product.set_stock(new_stock)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product::QuantityUnit;

    fn milk(stock: f64) -> Product {
        Product::new("Milk", QuantityUnit::Millilitres { step: 1000.0 }, stock, 1000.0, None)
            .expect("valid product")
    }

    #[test]
    fn consume_reduces_stock() {
        let p = milk(2000.0);
        assert_eq!(consume(&p, 500.0), Ok(1500.0));
    }

    #[test]
    fn consume_to_exactly_zero_is_allowed() {
        let p = milk(1000.0);
        assert_eq!(consume(&p, 1000.0), Ok(0.0));
    }

    #[test]
    fn over_consume_is_an_error_not_a_negative() {
        let p = milk(500.0);
        assert_eq!(
            consume(&p, 800.0),
            Err(StockError::NotEnoughStock { wanted: 800.0, available: 500.0 })
        );
    }

    #[test]
    fn purchase_increases_stock() {
        let p = milk(500.0);
        assert_eq!(purchase(&p, 1000.0), Ok(1500.0));
    }

    #[test]
    fn open_does_not_change_stock_but_needs_a_package() {
        let p = milk(1000.0);
        assert_eq!(open(&p, 1000.0), Ok(1000.0));
        let empty = milk(0.0);
        assert!(matches!(open(&empty, 1000.0), Err(StockError::NotEnoughStock { .. })));
    }

    #[test]
    fn bad_amounts_are_rejected() {
        let p = milk(1000.0);
        assert_eq!(consume(&p, f64::NAN), Err(StockError::BadAmount));
        assert_eq!(purchase(&p, -1.0), Err(StockError::BadAmount));
    }

    #[test]
    fn apply_commits_a_computed_amount() {
        let mut p = milk(2000.0);
        let after = consume(&p, 500.0).unwrap();
        apply(&mut p, after).unwrap();
        assert_eq!(p.stock(), 1500.0);
    }
}
