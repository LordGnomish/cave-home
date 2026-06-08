//! The shopping-list engine.
//!
//! Two sources feed one list: products that have fallen below their minimum
//! stock ([`below_min`]) and items the household typed in by hand
//! ([`ShoppingItem::manual`]). [`merge`] combines them, summing amounts for the
//! same product so "buy 2 milk (auto)" and "buy 1 milk (manual)" become one
//! "buy 3 milk" line. Every auto amount is rounded *up* to a whole purchasable
//! unit so the household never comes home short.

use crate::label::Lang;
use crate::product::{Product, QuantityUnit};

/// One line on the shopping list.
#[derive(Debug, Clone, PartialEq)]
pub struct ShoppingItem {
    name: String,
    /// How much to buy, already rounded up to whole purchase units.
    amount: f64,
    unit: QuantityUnit,
    /// `true` if the household added this by hand rather than the stock trigger.
    manual: bool,
}

impl ShoppingItem {
    /// A hand-added item ("we're out of birthday candles").
    #[must_use]
    pub fn manual(name: impl Into<String>, amount: f64, unit: QuantityUnit) -> Self {
        Self {
            name: name.into(),
            amount: amount.max(0.0),
            unit,
            manual: true,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn amount(&self) -> f64 {
        self.amount
    }

    #[must_use]
    pub const fn unit(&self) -> QuantityUnit {
        self.unit
    }

    #[must_use]
    pub const fn is_manual(&self) -> bool {
        self.manual
    }

    /// A grandma-friendly one-liner: "Milk — buy 3 ml" stays plain language.
    #[must_use]
    pub fn line(&self, lang: Lang) -> String {
        let verb = match lang {
            Lang::En => "buy",
            Lang::De => "kaufen",
            Lang::Tr => "al",
        };
        let unit = self.unit.label(lang);
        let qty = trim_amount(self.amount);
        match lang {
            // German/Turkish read "Milch: 3 Stk. kaufen" / "Süt: 3 adet al".
            Lang::En => format!("{}: {verb} {qty} {unit}", self.name),
            Lang::De | Lang::Tr => format!("{}: {qty} {unit} {verb}", self.name),
        }
    }
}

/// Round `shortfall` up to a whole number of purchasable units for `unit`.
#[must_use]
fn round_up_to_unit(shortfall: f64, unit: QuantityUnit) -> f64 {
    let step = unit.purchase_step();
    if shortfall <= 0.0 {
        return 0.0;
    }
    (shortfall / step).ceil() * step
}

/// The items that need buying because they fell below their minimum stock.
///
/// The amount on each line is `min_stock - stock`, rounded up to a whole
/// purchase unit. Products at or above their minimum produce no line.
#[must_use]
pub fn below_min(products: &[Product]) -> Vec<ShoppingItem> {
    products
        .iter()
        .filter(|p| p.is_below_min())
        .map(|p| {
            let shortfall = p.min_stock() - p.stock();
            ShoppingItem {
                name: p.name().to_owned(),
                amount: round_up_to_unit(shortfall, p.unit()),
                unit: p.unit(),
                manual: false,
            }
        })
        .collect()
}

/// Merge an auto-generated list with hand-added items.
///
/// Lines for the same product name (case-insensitive) are combined, their
/// amounts summed. A merged line is marked manual only if *every* contributing
/// line was manual, so a product that is both low *and* hand-added still reads
/// as an automatic suggestion.
#[must_use]
pub fn merge(auto: Vec<ShoppingItem>, manual: Vec<ShoppingItem>) -> Vec<ShoppingItem> {
    let mut out: Vec<ShoppingItem> = Vec::new();
    for item in auto.into_iter().chain(manual) {
        if let Some(existing) = out
            .iter_mut()
            .find(|e| e.name.eq_ignore_ascii_case(&item.name))
        {
            existing.amount += item.amount;
            existing.manual = existing.manual && item.manual;
        } else {
            out.push(item);
        }
    }
    out
}

/// Trim a float to a tidy household string: "3" not "3", "1.5" not "1.5000".
fn trim_amount(v: f64) -> String {
    if (v.fract()).abs() < f64::EPSILON {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, unit: QuantityUnit, stock: f64, min: f64) -> Product {
        Product::new(name, unit, stock, min, None).expect("valid product")
    }

    #[test]
    fn below_min_lists_only_short_products() {
        let products = [
            p("Milk", QuantityUnit::Millilitres { step: 1000.0 }, 500.0, 2000.0),
            p("Eggs", QuantityUnit::Piece, 12.0, 6.0), // plenty
        ];
        let list = below_min(&products);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name(), "Milk");
    }

    #[test]
    fn below_min_rounds_up_to_purchase_units() {
        // Short by 1500 ml, sold in 1000 ml cartons -> must buy 2000 ml.
        let products = [p(
            "Milk",
            QuantityUnit::Millilitres { step: 1000.0 },
            500.0,
            2000.0,
        )];
        assert_eq!(below_min(&products)[0].amount(), 2000.0);
    }

    #[test]
    fn below_min_rounds_packs_up() {
        // Short by 4 cans, sold in six-packs -> one six-pack.
        let products = [p("Cola", QuantityUnit::Pack(6), 2.0, 6.0)];
        assert_eq!(below_min(&products)[0].amount(), 6.0);
    }

    #[test]
    fn merge_sums_the_same_product() {
        let auto = vec![ShoppingItem {
            name: "Milk".into(),
            amount: 2000.0,
            unit: QuantityUnit::Millilitres { step: 1000.0 },
            manual: false,
        }];
        let manual = vec![ShoppingItem::manual(
            "milk",
            1000.0,
            QuantityUnit::Millilitres { step: 1000.0 },
        )];
        let merged = merge(auto, manual);
        assert_eq!(merged.len(), 1, "case-insensitive same product collapses");
        assert_eq!(merged[0].amount(), 3000.0);
        assert!(!merged[0].is_manual(), "auto+manual reads as auto");
    }

    #[test]
    fn merge_keeps_distinct_products_separate() {
        let auto = below_min(&[p("Eggs", QuantityUnit::Piece, 1.0, 6.0)]);
        let manual = vec![ShoppingItem::manual("Candles", 10.0, QuantityUnit::Piece)];
        let merged = merge(auto, manual);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn lines_are_plain_language() {
        let item = ShoppingItem::manual("Milk", 3.0, QuantityUnit::Piece);
        assert_eq!(item.line(Lang::En), "Milk: buy 3 pcs");
        assert_eq!(item.line(Lang::De), "Milk: 3 Stk. kaufen");
        assert_eq!(item.line(Lang::Tr), "Milk: 3 adet al");
    }
}
