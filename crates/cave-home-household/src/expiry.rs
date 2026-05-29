//! Expiry tracking — classify products against the household's "today".
//!
//! There is no clock: the caller supplies `today` as an integer day number on
//! the same scale as a product's best-before day. A product is [`Freshness::Fresh`]
//! if its best-before is comfortably ahead, [`Freshness::ExpiringSoon`] if it
//! falls within the warning window, and [`Freshness::Expired`] once today has
//! passed it. [`report`] rolls a basket up into an [`ExpiryReport`] the Portal
//! can show.

use crate::label::Lang;
use crate::product::Product;

/// How close a product is to its best-before day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Freshness {
    /// Best-before is more than the warning window away (or unset).
    Fresh,
    /// Best-before is today or within the warning window.
    ExpiringSoon,
    /// Best-before is in the past — already expired.
    Expired,
}

impl Freshness {
    /// Classify one best-before day against `today` with a `within` warning
    /// window (in days). A product with no best-before is always [`Self::Fresh`].
    #[must_use]
    pub const fn classify(best_before: Option<i64>, today: i64, within: i64) -> Self {
        match best_before {
            None => Self::Fresh,
            Some(bb) => {
                let days_left = bb - today;
                if days_left < 0 {
                    Self::Expired
                } else if days_left <= within {
                    Self::ExpiringSoon
                } else {
                    Self::Fresh
                }
            }
        }
    }
}

/// One product's expiry verdict, carrying enough to phrase a plain-language line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryEntry {
    name: String,
    freshness: Freshness,
    /// Days until best-before (negative = days past it); `None` if no date set.
    days_left: Option<i64>,
}

impl ExpiryEntry {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }

    #[must_use]
    pub const fn days_left(&self) -> Option<i64> {
        self.days_left
    }

    /// A grandma-friendly line: "Yogurt expires tomorrow", "Milk has gone off".
    #[must_use]
    pub fn line(&self, lang: Lang) -> String {
        match (self.freshness, self.days_left) {
            (Freshness::Expired, _) => match lang {
                Lang::En => format!("{} has gone off — throw it out", self.name),
                Lang::De => format!("{} ist abgelaufen — wegwerfen", self.name),
                Lang::Tr => format!("{} bozulmuş — atın", self.name),
            },
            (Freshness::ExpiringSoon, Some(0)) => match lang {
                Lang::En => format!("{} is best used today", self.name),
                Lang::De => format!("{} heute am besten aufbrauchen", self.name),
                Lang::Tr => format!("{} en iyi bugün tüketilir", self.name),
            },
            (Freshness::ExpiringSoon, Some(1)) => match lang {
                Lang::En => format!("{} expires tomorrow", self.name),
                Lang::De => format!("{} läuft morgen ab", self.name),
                Lang::Tr => format!("{} yarın son gününde", self.name),
            },
            (Freshness::ExpiringSoon, Some(n)) => match lang {
                Lang::En => format!("{} is running out of time — {n} days left", self.name),
                Lang::De => format!("{} wird bald knapp — noch {n} Tage", self.name),
                Lang::Tr => format!("{} az kaldı — {n} gün var", self.name),
            },
            // Fresh or no date: nothing pressing to say.
            _ => match lang {
                Lang::En => format!("{} is fine", self.name),
                Lang::De => format!("{} ist in Ordnung", self.name),
                Lang::Tr => format!("{} taze", self.name),
            },
        }
    }
}

/// A whole basket's worth of expiry verdicts, grouped for the Portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryReport {
    /// Every entry, in input order.
    pub entries: Vec<ExpiryEntry>,
}

impl ExpiryReport {
    /// Entries that have already expired.
    #[must_use]
    pub fn expired(&self) -> Vec<&ExpiryEntry> {
        self.entries
            .iter()
            .filter(|e| e.freshness == Freshness::Expired)
            .collect()
    }

    /// Entries expiring within the warning window (but not yet expired).
    #[must_use]
    pub fn expiring_soon(&self) -> Vec<&ExpiryEntry> {
        self.entries
            .iter()
            .filter(|e| e.freshness == Freshness::ExpiringSoon)
            .collect()
    }

    /// `true` if nothing in the basket needs attention.
    #[must_use]
    pub fn all_fresh(&self) -> bool {
        self.entries.iter().all(|e| e.freshness == Freshness::Fresh)
    }
}

/// Build an [`ExpiryReport`] for `products` given the household's `today` and a
/// `within`-day warning window.
#[must_use]
pub fn report(products: &[Product], today: i64, within: i64) -> ExpiryReport {
    let entries = products
        .iter()
        .map(|p| ExpiryEntry {
            name: p.name().to_owned(),
            freshness: Freshness::classify(p.best_before(), today, within),
            days_left: p.best_before().map(|bb| bb - today),
        })
        .collect();
    ExpiryReport { entries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product::QuantityUnit;

    fn yogurt(bb: Option<i64>) -> Product {
        Product::new("Yogurt", QuantityUnit::Piece, 1.0, 0.0, bb).expect("valid")
    }

    #[test]
    fn classify_boundaries() {
        // today = 100, window = 3 days.
        assert_eq!(Freshness::classify(Some(110), 100, 3), Freshness::Fresh);
        // Exactly at the edge of the window is still "soon".
        assert_eq!(Freshness::classify(Some(103), 100, 3), Freshness::ExpiringSoon);
        // Just outside is fresh.
        assert_eq!(Freshness::classify(Some(104), 100, 3), Freshness::Fresh);
        // Today itself is "soon", not expired.
        assert_eq!(Freshness::classify(Some(100), 100, 3), Freshness::ExpiringSoon);
        // Yesterday is expired.
        assert_eq!(Freshness::classify(Some(99), 100, 3), Freshness::Expired);
    }

    #[test]
    fn no_best_before_is_always_fresh() {
        assert_eq!(Freshness::classify(None, 100, 3), Freshness::Fresh);
    }

    #[test]
    fn report_groups_by_freshness() {
        let products = [
            yogurt(Some(99)),  // expired
            yogurt(Some(101)), // soon (window 3)
            yogurt(Some(200)), // fresh
            yogurt(None),      // fresh
        ];
        let r = report(&products, 100, 3);
        assert_eq!(r.expired().len(), 1);
        assert_eq!(r.expiring_soon().len(), 1);
        assert!(!r.all_fresh());
    }

    #[test]
    fn report_all_fresh_when_nothing_pressing() {
        let products = [yogurt(None), yogurt(Some(500))];
        assert!(report(&products, 100, 3).all_fresh());
    }

    #[test]
    fn lines_are_plain_language() {
        let r = report(&[yogurt(Some(101))], 100, 3);
        assert_eq!(r.entries[0].line(Lang::En), "Yogurt expires tomorrow");
        let gone = report(&[yogurt(Some(50))], 100, 3);
        assert_eq!(gone.entries[0].line(Lang::De), "Yogurt ist abgelaufen — wegwerfen");
    }
}
