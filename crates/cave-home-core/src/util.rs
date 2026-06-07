//! Small ports of `homeassistant.util` helpers shared across the registries.
//!
//! [`slugify`] mirrors `homeassistant.util.slugify` closely enough for entity
//! / area id generation: lowercase, non-`[a-z0-9]` runs collapse to a single
//! `_`, leading/trailing `_` are stripped. [`ensure_unique_string`] mirrors
//! `homeassistant.util.ensure_unique_string`, the `_2`/`_3` suffixing the
//! entity and area registries use to avoid collisions.

use std::collections::HashSet;
use std::hash::BuildHasher;

/// Port of `homeassistant.util.slugify`.
///
/// Folds `value` to a lowercase `[a-z0-9_]` slug. Any maximal run of
/// characters outside that class becomes a single `_`; leading and trailing
/// underscores are trimmed. An all-separator input slugs to the empty string.
#[must_use]
pub fn slugify(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_sep = false;
    for ch in value.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_lowercase() || lowered.is_ascii_digit() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            pending_sep = false;
            out.push(lowered);
        } else {
            // a separator run — remember we owe at most one underscore, but
            // only emit it once we know a kept char follows (no trailing `_`).
            pending_sep = true;
        }
    }
    out
}

/// Port of `homeassistant.util.ensure_unique_string`.
///
/// Returns `preferred` if it is not already in `existing`; otherwise appends
/// `_2`, `_3`, … until an unused candidate is found.
#[must_use]
pub fn ensure_unique_string<S: BuildHasher>(preferred: &str, existing: &HashSet<String, S>) -> String {
    if !existing.contains(preferred) {
        return preferred.to_owned();
    }
    let mut tries = 2u32;
    loop {
        let candidate = format!("{preferred}_{tries}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        tries += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_collapses_separators() {
        assert_eq!(slugify("Kitchen Light"), "kitchen_light");
        assert_eq!(slugify("Front  Door!!"), "front_door");
        assert_eq!(slugify("  Living-Room  "), "living_room");
        assert_eq!(slugify("Temp #2"), "temp_2");
        // already a slug → unchanged
        assert_eq!(slugify("binary_sensor"), "binary_sensor");
        // all separators → empty
        assert_eq!(slugify("---"), "");
        // unicode/punctuation folded out
        assert_eq!(slugify("Büro 1"), "b_ro_1");
    }

    #[test]
    fn ensure_unique_string_suffixes_on_collision() {
        let mut existing = HashSet::new();
        assert_eq!(ensure_unique_string("light_kitchen", &existing), "light_kitchen");
        existing.insert("light_kitchen".to_owned());
        assert_eq!(ensure_unique_string("light_kitchen", &existing), "light_kitchen_2");
        existing.insert("light_kitchen_2".to_owned());
        assert_eq!(ensure_unique_string("light_kitchen", &existing), "light_kitchen_3");
    }
}
