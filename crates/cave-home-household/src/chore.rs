//! Chores — recurring household tasks and who they belong to.
//!
//! Like the rest of the crate there is no clock: a [`Chore`] knows its period
//! in days and the day number it was last done, and the caller asks "given that
//! today is day N, is this due?". A chore can be assigned to a household member;
//! [`due_chores`] sweeps a list and returns everything that needs doing, so a
//! Phase 1b calendar/notify integration can turn each into a reminder.

use crate::label::Lang;

/// A recurring task: water the plants, change the smoke-detector battery, take
/// the bins out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chore {
    name: String,
    /// How often it recurs, in days. A period of 0 means "always due".
    period_days: i64,
    /// The day number it was last completed; `None` if never done.
    last_done: Option<i64>,
    /// The household member it is assigned to, if anyone.
    assignee: Option<String>,
}

impl Chore {
    /// Define a chore. `period_days` is clamped to a non-negative value.
    #[must_use]
    pub fn new(name: impl Into<String>, period_days: i64, last_done: Option<i64>) -> Self {
        Self {
            name: name.into(),
            period_days: period_days.max(0),
            last_done,
            assignee: None,
        }
    }

    /// Assign this chore to a household member (builder-style).
    #[must_use]
    pub fn assigned_to(mut self, who: impl Into<String>) -> Self {
        self.assignee = Some(who.into());
        self
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn period_days(&self) -> i64 {
        self.period_days
    }

    #[must_use]
    pub const fn last_done(&self) -> Option<i64> {
        self.last_done
    }

    #[must_use]
    pub fn assignee(&self) -> Option<&str> {
        self.assignee.as_deref()
    }

    /// The day number this chore next falls due.
    ///
    /// A chore that has never been done is due immediately (returns `today`'s
    /// floor of `None` semantics via [`Self::is_due`]); here we report the
    /// scheduled day for one that *has* been done: `last_done + period`.
    #[must_use]
    pub const fn next_due(&self) -> Option<i64> {
        match self.last_done {
            Some(last) => Some(last + self.period_days),
            None => None,
        }
    }

    /// Is this chore due on day `today`?
    ///
    /// Never-done chores are always due. A done chore is due once `today`
    /// reaches `last_done + period_days`.
    #[must_use]
    pub const fn is_due(&self, today: i64) -> bool {
        match self.next_due() {
            None => true,
            Some(due_day) => today >= due_day,
        }
    }

    /// How many days overdue (positive) or remaining (negative) on `today`.
    /// `None` for a never-done chore (it is simply due now).
    #[must_use]
    pub fn days_overdue(&self, today: i64) -> Option<i64> {
        self.next_due().map(|due| today - due)
    }

    /// A grandma-friendly reminder line: "Time to water the plants — chore due".
    #[must_use]
    pub fn reminder(&self, today: i64, lang: Lang) -> String {
        if !self.is_due(today) {
            return match lang {
                Lang::En => format!("{} is up to date", self.name),
                Lang::De => format!("{} ist erledigt", self.name),
                Lang::Tr => format!("{} güncel", self.name),
            };
        }
        let who = self.assignee.as_deref();
        match (lang, who) {
            (Lang::En, Some(p)) => format!("Time for {p} to do: {} — chore due", self.name),
            (Lang::En, None) => format!("Time to do: {} — chore due", self.name),
            (Lang::De, Some(p)) => format!("{p} ist dran: {} — fällig", self.name),
            (Lang::De, None) => format!("Zeit für: {} — fällig", self.name),
            (Lang::Tr, Some(p)) => format!("Sıra {p} kişisinde: {} — zamanı geldi", self.name),
            (Lang::Tr, None) => format!("Yapma zamanı: {} — zamanı geldi", self.name),
        }
    }
}

/// Every chore that is due on `today`, in input order.
#[must_use]
pub fn due_chores(chores: &[Chore], today: i64) -> Vec<&Chore> {
    chores.iter().filter(|c| c.is_due(today)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_done_is_due_now() {
        let c = Chore::new("Water the plants", 7, None);
        assert!(c.is_due(0));
        assert!(c.is_due(1000));
        assert_eq!(c.next_due(), None);
    }

    #[test]
    fn next_due_is_last_plus_period() {
        let c = Chore::new("Change smoke-detector battery", 365, Some(100));
        assert_eq!(c.next_due(), Some(465));
    }

    #[test]
    fn due_math_at_the_boundary() {
        let c = Chore::new("Water the plants", 7, Some(10));
        assert!(!c.is_due(16), "day before due is not due");
        assert!(c.is_due(17), "exactly on the due day is due");
        assert!(c.is_due(20), "past due is due");
    }

    #[test]
    fn days_overdue_counts_correctly() {
        let c = Chore::new("Bins", 7, Some(10));
        assert_eq!(c.days_overdue(20), Some(3)); // due day 17, 3 days late
        assert_eq!(c.days_overdue(15), Some(-2)); // 2 days early
    }

    #[test]
    fn due_chores_filters_the_list() {
        let chores = [
            Chore::new("Water plants", 7, Some(10)), // due day 17
            Chore::new("Vacuum", 14, Some(15)),      // due day 29
            Chore::new("New chore", 7, None),        // due now
        ];
        let due = due_chores(&chores, 18);
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].name(), "Water plants");
        assert_eq!(due[1].name(), "New chore");
    }

    #[test]
    fn assignment_is_carried_and_named() {
        let c = Chore::new("Water the plants", 7, None).assigned_to("Ada");
        assert_eq!(c.assignee(), Some("Ada"));
        assert!(c.reminder(0, Lang::En).contains("Ada"));
    }

    #[test]
    fn reminders_are_plain_language() {
        let due = Chore::new("Water the plants", 7, None);
        assert_eq!(due.reminder(0, Lang::En), "Time to do: Water the plants — chore due");
        let done = Chore::new("Water the plants", 7, Some(100));
        assert_eq!(done.reminder(101, Lang::Tr), "Water the plants güncel");
    }
}
