//! Recipe stock-check — "can we make dinner tonight?".
//!
//! A [`Recipe`] is a list of ingredients, each a product name and the amount it
//! needs. [`can_make`] compares it against the current basket and answers
//! yes/no plus the shortfall — and that shortfall comes back as
//! [`crate::shopping::ShoppingItem`]s ready to merge into the shopping list, each
//! rounded up to whole purchase units.

use crate::label::Lang;
use crate::product::Product;
use crate::shopping::ShoppingItem;

/// One ingredient line: how much of a named product the recipe needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Ingredient {
    name: String,
    amount: f64,
}

impl Ingredient {
    /// An ingredient requirement. A negative/non-finite amount is clamped to 0.
    #[must_use]
    pub fn new(name: impl Into<String>, amount: f64) -> Self {
        Self {
            name: name.into(),
            amount: if amount.is_finite() { amount.max(0.0) } else { 0.0 },
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
}

/// A recipe: a name and the ingredients it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    name: String,
    ingredients: Vec<Ingredient>,
}

impl Recipe {
    #[must_use]
    pub fn new(name: impl Into<String>, ingredients: Vec<Ingredient>) -> Self {
        Self {
            name: name.into(),
            ingredients,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn ingredients(&self) -> &[Ingredient] {
        &self.ingredients
    }
}

/// The verdict on whether the household can make a recipe right now.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeCheck {
    can_make: bool,
    /// What is missing, as shopping lines (empty if we can make it).
    shortfall: Vec<ShoppingItem>,
}

impl RecipeCheck {
    #[must_use]
    pub const fn can_make(&self) -> bool {
        self.can_make
    }

    #[must_use]
    pub fn shortfall(&self) -> &[ShoppingItem] {
        &self.shortfall
    }

    /// Hand back the shortfall lines for merging into the shopping list.
    #[must_use]
    pub fn into_shortfall(self) -> Vec<ShoppingItem> {
        self.shortfall
    }

    /// A grandma-friendly verdict line.
    #[must_use]
    pub fn summary(&self, recipe: &Recipe, lang: Lang) -> String {
        if self.can_make {
            return match lang {
                Lang::En => format!("You have everything for {}", recipe.name()),
                Lang::De => format!("Alles für {} ist da", recipe.name()),
                Lang::Tr => format!("{} için her şey var", recipe.name()),
            };
        }
        let missing = self.shortfall.len();
        match lang {
            Lang::En => format!("{} needs {missing} more thing(s) from the shop", recipe.name()),
            Lang::De => format!("Für {} fehlen noch {missing} Sache(n)", recipe.name()),
            Lang::Tr => format!("{} için {missing} şey eksik", recipe.name()),
        }
    }
}

/// Can the household make `recipe` from the current `basket`?
///
/// For each ingredient we look up a product by name (case-insensitive). A
/// missing product, or one with too little stock, becomes a shortfall line for
/// the difference, rounded up to whole purchase units. The recipe is makeable
/// only if every ingredient is fully covered.
#[must_use]
pub fn can_make(recipe: &Recipe, basket: &[Product]) -> RecipeCheck {
    let mut shortfall = Vec::new();
    for ingredient in &recipe.ingredients {
        let stocked = basket
            .iter()
            .find(|p| p.name().eq_ignore_ascii_case(ingredient.name()));
        let have = stocked.map_or(0.0, Product::stock);
        let missing = ingredient.amount() - have;
        if missing > 0.0 {
            // Round up to a purchase unit if we know the product's unit;
            // otherwise fall back to a plain piece count.
            let item = stocked.map_or_else(
                || {
                    ShoppingItem::manual(
                        ingredient.name(),
                        missing.ceil(),
                        crate::product::QuantityUnit::Piece,
                    )
                },
                |p| {
                    let step = p.unit().purchase_step();
                    let amount = (missing / step).ceil() * step;
                    ShoppingItem::manual(ingredient.name(), amount, p.unit())
                },
            );
            shortfall.push(item);
        }
    }
    RecipeCheck {
        can_make: shortfall.is_empty(),
        shortfall,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product::QuantityUnit;

    fn basket() -> Vec<Product> {
        vec![
            Product::new("Eggs", QuantityUnit::Piece, 6.0, 0.0, None).unwrap(),
            Product::new("Flour", QuantityUnit::Grams { step: 500.0 }, 1000.0, 0.0, None).unwrap(),
            Product::new("Milk", QuantityUnit::Millilitres { step: 1000.0 }, 200.0, 0.0, None)
                .unwrap(),
        ]
    }

    #[test]
    fn can_make_when_everything_is_in_stock() {
        let pancakes = Recipe::new(
            "Pancakes",
            vec![
                Ingredient::new("Eggs", 3.0),
                Ingredient::new("Flour", 300.0),
                Ingredient::new("Milk", 200.0),
            ],
        );
        let check = can_make(&pancakes, &basket());
        assert!(check.can_make());
        assert!(check.shortfall().is_empty());
    }

    #[test]
    fn shortfall_lists_what_is_missing_rounded_up() {
        let pancakes = Recipe::new(
            "Pancakes",
            vec![
                Ingredient::new("Eggs", 10.0),  // have 6, short 4
                Ingredient::new("Milk", 500.0), // have 200, short 300 -> 1 carton (1000)
            ],
        );
        let check = can_make(&pancakes, &basket());
        assert!(!check.can_make());
        assert_eq!(check.shortfall().len(), 2);
        let eggs = &check.shortfall()[0];
        assert_eq!(eggs.name(), "Eggs");
        assert_eq!(eggs.amount(), 4.0);
        let milk = &check.shortfall()[1];
        assert_eq!(milk.amount(), 1000.0, "300 ml short rounds up to a 1 L carton");
    }

    #[test]
    fn missing_product_entirely_is_a_shortfall() {
        let recipe = Recipe::new("Omelette", vec![Ingredient::new("Cheese", 2.0)]);
        let check = can_make(&recipe, &basket());
        assert!(!check.can_make());
        assert_eq!(check.shortfall()[0].name(), "Cheese");
        assert_eq!(check.shortfall()[0].amount(), 2.0);
    }

    #[test]
    fn summary_is_plain_language() {
        let recipe = Recipe::new("Pancakes", vec![Ingredient::new("Eggs", 2.0)]);
        let ok = can_make(&recipe, &basket());
        assert_eq!(ok.summary(&recipe, Lang::En), "You have everything for Pancakes");
    }
}
