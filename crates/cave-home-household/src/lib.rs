//! `cave-home-household` — household-management intelligence for cave-home
//! (ADR-026, Grocy-class semantics).
//!
//! This crate is the **brain** that keeps a household's pantry, chores and
//! shopping in order: it tracks what is in stock and moves it in and out, builds
//! the shopping list from whatever has run low (merged with hand-added items),
//! flags food that is about to go off, says which recurring chores are due, and
//! checks whether tonight's recipe can actually be made — all in plain language
//! in EN / DE / TR.
//!
//! There is **no clock and no network**: the caller supplies "today" as an
//! integer day number, so every function here is pure and deterministic. The
//! domain model is implemented first-party from Grocy-class *semantics* (food /
//! medicine / battery inventory, chores, shopping list, recipes) — no Grocy PHP
//! source was ported.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`product`] — the validated inventory-entry model + purchase units.
//! - [`stock`] — `consume` / `purchase` / `open` as pure, over-draw-safe ops.
//! - [`shopping`] — the below-min shopping engine + manual-item merge + unit rounding.
//! - [`expiry`] — Fresh / `ExpiringSoon` / Expired classification + report.
//! - [`chore`] — recurring chores: due / next-due math, assignment, due-list.
//! - [`recipe`] — "can we make it?" + shortfall shopping list.
//! - [`label`] — the localisation surface (Charter §6.3, ADR-007).
//!
//! **Barcode lookup + product-database import**, the **Grocy REST API-compat
//! layer**, **persistent storage**, and **cave-home-core / cave-home-calendar
//! integration** (chore reminders as calendar/notify events) are
//! storage/network-bound and deferred to Phase 1b — each is enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-026 disposition. They feed
//! their inputs into this engine and reuse it unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_household::{
//!     below_min, merge, Product, QuantityUnit, ShoppingItem, Lang,
//! };
//!
//! // Milk has run low: 0.5 L on hand, we like to keep 2 L.
//! let milk = Product::new(
//!     "Milk",
//!     QuantityUnit::Millilitres { step: 1000.0 },
//!     500.0,
//!     2000.0,
//!     None,
//! )
//! .unwrap();
//!
//! let auto = below_min(&[milk]);
//! // Short by 1.5 L, sold in 1 L cartons -> buy two cartons (2000 ml).
//! assert_eq!(auto[0].amount(), 2000.0);
//!
//! // Merge with a hand-added item; the household reads a plain-language list.
//! let manual = vec![ShoppingItem::manual("Bread", 1.0, QuantityUnit::Piece)];
//! let list = merge(auto, manual);
//! assert_eq!(list.len(), 2);
//! println!("{}", list[0].line(Lang::En)); // "Milk: buy 2000 ml"
//! ```

pub mod chore;
pub mod expiry;
pub mod label;
pub mod product;
pub mod recipe;
pub mod shopping;
pub mod stock;

pub use chore::{due_chores, Chore};
pub use expiry::{report, ExpiryEntry, ExpiryReport, Freshness};
pub use label::Lang;
pub use product::{Product, ProductError, QuantityUnit};
pub use recipe::{can_make, Ingredient, Recipe, RecipeCheck};
pub use shopping::{below_min, merge, ShoppingItem};
pub use stock::{consume, open, purchase, StockError};

#[cfg(test)]
mod tests {
    use super::*;

    /// Charter §6.3: no implementation jargon may leak into any user-facing
    /// string this crate produces. We exercise every localised surface in all
    /// three languages and assert none contains a banned term. Mirrors the
    /// air-quality crate's `ui_strings_carry_no_implementation_jargon`.
    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        const BANNED: &[&str] = &[
            "Grocy", "SKU", "foreign key", "stock entry", "REST", "endpoint",
            "MQTT", "entity_id", "Zigbee", "Z-Wave", "barcode", "database",
            "PHP", "pod", "kubelet",
        ];
        let langs = Lang::ALL;
        let mut strings: Vec<String> = Vec::new();

        for lang in langs {
            // Shopping lines (auto + manual).
            let milk = Product::new(
                "Milk",
                QuantityUnit::Millilitres { step: 1000.0 },
                500.0,
                2000.0,
                None,
            )
            .expect("valid product");
            for item in below_min(&[milk]) {
                strings.push(item.line(lang));
            }
            strings.push(ShoppingItem::manual("Bread", 1.0, QuantityUnit::Piece).line(lang));

            // Expiry lines across every freshness state.
            let basket = [
                Product::new("Yogurt", QuantityUnit::Piece, 1.0, 0.0, Some(101)).unwrap(),
                Product::new("Cream", QuantityUnit::Piece, 1.0, 0.0, Some(100)).unwrap(),
                Product::new("Cheese", QuantityUnit::Piece, 1.0, 0.0, Some(50)).unwrap(),
                Product::new("Honey", QuantityUnit::Piece, 1.0, 0.0, None).unwrap(),
                Product::new("Ham", QuantityUnit::Piece, 1.0, 0.0, Some(105)).unwrap(),
            ];
            for entry in report(&basket, 100, 3).entries {
                strings.push(entry.line(lang));
            }

            // Chore reminders (due, not due, assigned).
            let due = Chore::new("Water the plants", 7, None);
            let assigned = Chore::new("Take the bins out", 7, None).assigned_to("Ada");
            let done = Chore::new("Vacuum", 7, Some(100));
            strings.push(due.reminder(0, lang));
            strings.push(assigned.reminder(0, lang));
            strings.push(done.reminder(101, lang));

            // Recipe verdicts (makeable + short).
            let recipe = Recipe::new(
                "Pancakes",
                vec![Ingredient::new("Eggs", 99.0), Ingredient::new("Flour", 1.0)],
            );
            let stock = [Product::new("Eggs", QuantityUnit::Piece, 2.0, 0.0, None).unwrap()];
            let check = can_make(&recipe, &stock);
            strings.push(check.summary(&recipe, lang));
            for item in check.shortfall() {
                strings.push(item.line(lang));
            }
        }

        for text in &strings {
            for banned in BANNED {
                assert!(
                    !text.to_lowercase().contains(&banned.to_lowercase()),
                    "user-facing string leaks jargon {banned:?}: {text:?}"
                );
            }
        }
    }

    /// An end-to-end sweep: low milk + an expiring item + a due chore + a recipe
    /// we cannot quite make, all flowing through the public surface.
    #[test]
    fn engine_runs_a_whole_household_day() {
        // Pantry: milk low, yogurt about to turn, eggs plenty.
        let milk = Product::new(
            "Milk",
            QuantityUnit::Millilitres { step: 1000.0 },
            300.0,
            2000.0,
            Some(105),
        )
        .unwrap();
        let yogurt = Product::new("Yogurt", QuantityUnit::Piece, 4.0, 2.0, Some(101)).unwrap();
        let eggs = Product::new("Eggs", QuantityUnit::Piece, 12.0, 6.0, None).unwrap();
        let pantry = [milk.clone(), yogurt, eggs];

        // Shopping list: only milk is low.
        let auto = below_min(&pantry);
        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].name(), "Milk");
        assert_eq!(auto[0].amount(), 2000.0); // short 1700 -> two 1 L cartons

        // We buy it; stock crosses back above the minimum.
        let restocked = purchase(&milk, 2000.0).unwrap();
        assert_eq!(restocked, 2300.0);

        // Expiry: yogurt is the only thing flagged (today = 100, window 3).
        let report = report(&pantry, 100, 3);
        assert_eq!(report.expiring_soon().len(), 1);
        assert_eq!(report.expiring_soon()[0].name(), "Yogurt");

        // A weekly chore last done day 90 is due on day 100.
        let chore = Chore::new("Water the plants", 7, Some(90));
        assert!(chore.is_due(100));
        assert_eq!(chore.next_due(), Some(97));

        // Pancakes need 3 eggs + 1 L milk; the fresh stock can't quite cover milk.
        let recipe = Recipe::new(
            "Pancakes",
            vec![Ingredient::new("Eggs", 3.0), Ingredient::new("Milk", 1000.0)],
        );
        let check = can_make(&recipe, &pantry);
        assert!(!check.can_make(), "only 300 ml milk on hand");
        assert_eq!(check.shortfall()[0].name(), "Milk");
    }
}
