// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The honest answer for a verb whose backend is not wired into the CLI yet.
//!
//! Charter §6.3: the first-rank user is the non-technical family member, who
//! believes what the screen says. A verb that cannot reach the device or the
//! hub therefore never prints a result (no demo data, no "done") — it says, in
//! Turkish first and English second, that nothing was sent and nothing
//! changed, writes nothing to stdout, and exits [`EXIT_NOT_CONNECTED`] so a
//! script cannot mistake it for success either.

/// Exit code for "this verb's backend is not connected" — sysexits.h
/// `EX_UNAVAILABLE`, distinct from 1 (the operation failed) and 2 (usage).
pub const EXIT_NOT_CONNECTED: i32 = 69;

/// The two-line message for `what` (e.g. `"unifi access lockdown"`).
#[must_use]
pub fn message(what: &str) -> String {
    format!(
        "{what}: henüz bağlı değil — hiçbir cihaza ulaşılmadı, hiçbir şey değişmedi.\n\
         {what}: not connected yet — nothing was sent and nothing changed.\n"
    )
}

/// Print [`message`] to stderr and return [`EXIT_NOT_CONNECTED`].
#[must_use]
pub fn not_connected(what: &str) -> i32 {
    eprint!("{}", message(what));
    EXIT_NOT_CONNECTED
}

/// [`not_connected`] plus a line telling the household where to do it
/// instead (Turkish, then English).
#[must_use]
pub fn not_connected_with_hint(what: &str, hint_tr: &str, hint_en: &str) -> i32 {
    eprint!("{}", message(what));
    eprintln!("  {hint_tr}");
    eprintln!("  {hint_en}");
    EXIT_NOT_CONNECTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_is_turkish_first_and_names_the_verb() {
        let m = message("kilit");
        let first = m.lines().next().unwrap();
        assert!(first.starts_with("kilit: henüz bağlı değil"));
        assert!(m.contains("nothing was sent and nothing changed"));
    }

    #[test]
    fn exit_code_is_not_success_or_usage() {
        assert_ne!(EXIT_NOT_CONNECTED, 0);
        assert_ne!(EXIT_NOT_CONNECTED, 1);
        assert_ne!(EXIT_NOT_CONNECTED, 2);
        assert_eq!(not_connected("x"), EXIT_NOT_CONNECTED);
    }
}
