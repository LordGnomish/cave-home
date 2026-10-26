//! The playback-queue engine — the core of cave-home's music brain (ADR-020).
//!
//! A [`Queue`] is an ordered list of track ids plus a cursor (the "now playing"
//! position), a [`RepeatMode`], and an optional shuffle order. Everything a
//! person can do to a queue — enqueue, play this next, play now, clear, move a
//! song, remove a song, skip forward/back, toggle repeat, shuffle — is a pure
//! transformation here, with no audio device and no network in sight (the
//! actual sound is Snapcast's job, ADR-020).
//!
//! "Next" and "previous" are computed from three things together: the cursor,
//! the [`RepeatMode`], and (if on) the shuffle order. This is the part that is
//! tested hardest.

use crate::media::TrackId;
use crate::shuffle::shuffled_order;

/// What happens at the end (or start) of the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    /// Stop when the last song ends.
    #[default]
    Off,
    /// Replay the current song forever.
    One,
    /// Loop back to the first song after the last.
    All,
}

/// An ordered playback queue with a cursor.
///
/// The cursor is `None` when nothing is selected (a fresh or cleared queue) and
/// `Some(i)` to mean "track at play-position `i` is current". Positions are in
/// *play order*: with shuffle off that is the underlying order; with shuffle on
/// it indexes the shuffle permutation.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    /// The tracks, in the order they were enqueued.
    items: Vec<TrackId>,
    /// Cursor into *play order* (see [`Queue::play_order`]). `None` = nothing
    /// current.
    cursor: Option<usize>,
    repeat: RepeatMode,
    /// When `Some`, a permutation of `0..items.len()` giving the shuffled play
    /// order. `None` = play in stored order.
    shuffle: Option<Vec<usize>>,
}

impl Queue {
    /// A new, empty queue (repeat off, no shuffle).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many tracks are in the queue.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the queue holds no tracks.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The current repeat mode.
    #[must_use]
    pub const fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Set the repeat mode.
    pub const fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    /// Whether shuffle is currently on.
    #[must_use]
    pub const fn is_shuffled(&self) -> bool {
        self.shuffle.is_some()
    }

    /// The play order as underlying-item indices.
    ///
    /// With shuffle off this is `0..len`; with shuffle on it is the shuffle
    /// permutation. A defensive fallback to sequential order is used if a
    /// stored permutation ever has the wrong length (it never should).
    fn play_order(&self) -> Vec<usize> {
        match &self.shuffle {
            Some(order) if order.len() == self.items.len() => order.clone(),
            _ => (0..self.items.len()).collect(),
        }
    }

    /// Append items to the end of the queue ("add to queue").
    ///
    /// If shuffle is on, the new items extend the shuffle order at the end
    /// (they play after everything already shuffled). If the queue was empty
    /// and nothing was current, the cursor moves to the first new item.
    pub fn enqueue(&mut self, ids: impl IntoIterator<Item = TrackId>) {
        let before = self.items.len();
        self.items.extend(ids);
        let added = self.items.len() - before;
        if let Some(order) = &mut self.shuffle {
            order.extend(before..before + added);
        }
        if self.cursor.is_none() && !self.items.is_empty() {
            self.cursor = Some(0);
        }
    }

    /// Insert items so they play *right after* the current track ("play next").
    ///
    /// With nothing current it behaves like [`Queue::enqueue`]. Operates in play
    /// order, so it does the intuitive thing whether or not shuffle is on.
    pub fn enqueue_next(&mut self, ids: impl IntoIterator<Item = TrackId>) {
        let new: Vec<TrackId> = ids.into_iter().collect();
        if new.is_empty() {
            return;
        }
        let Some(cur_pos) = self.cursor else {
            self.enqueue(new);
            return;
        };
        // Rebuild as a flat play-order list, splice after the cursor, then drop
        // shuffle (the explicit "play next" intent overrides a shuffle order).
        let order = self.play_order();
        let mut flat: Vec<TrackId> = order.iter().map(|&i| self.items[i].clone()).collect();
        let insert_at = cur_pos + 1;
        for (offset, id) in new.into_iter().enumerate() {
            flat.insert(insert_at + offset, id);
        }
        self.items = flat;
        self.shuffle = None;
        self.cursor = Some(cur_pos);
    }

    /// Replace the whole queue with these items and start at the first ("play
    /// now"). Clears any shuffle order; keeps the repeat mode.
    pub fn play_now(&mut self, ids: impl IntoIterator<Item = TrackId>) {
        self.items = ids.into_iter().collect();
        self.shuffle = None;
        self.cursor = if self.items.is_empty() { None } else { Some(0) };
    }

    /// Empty the queue completely.
    pub fn clear(&mut self) {
        self.items.clear();
        self.shuffle = None;
        self.cursor = None;
    }

    /// The track id at the current cursor, if any.
    #[must_use]
    pub fn current(&self) -> Option<&TrackId> {
        let pos = self.cursor?;
        let item_idx = *self.play_order().get(pos)?;
        self.items.get(item_idx)
    }

    /// The current cursor position in play order, if any.
    #[must_use]
    pub const fn current_index(&self) -> Option<usize> {
        self.cursor
    }

    /// Move a track from one play-order position to another ("drag to
    /// reorder"). Out-of-range positions are ignored. Keeps the *same track*
    /// current across the move.
    pub fn move_item(&mut self, from: usize, to: usize) {
        let len = self.items.len();
        if from >= len || to >= len || from == to {
            return;
        }
        // Operate on the flat play order so positions match what the user sees.
        let current_track = self.current().cloned();
        let order = self.play_order();
        let mut flat: Vec<TrackId> = order.iter().map(|&i| self.items[i].clone()).collect();
        let moved = flat.remove(from);
        flat.insert(to, moved);
        self.items = flat;
        self.shuffle = None;
        self.cursor = current_track.and_then(|t| self.items.iter().position(|x| *x == t));
    }

    /// Remove the track at a play-order position ("remove from queue").
    ///
    /// Out-of-range positions are ignored. The cursor is kept pointing at the
    /// same surviving track where possible; removing the current track advances
    /// to what would have played next in linear order (or clamps to the end).
    pub fn remove_item(&mut self, pos: usize) {
        let len = self.items.len();
        if pos >= len {
            return;
        }
        let order = self.play_order();
        let Some(&victim_idx) = order.get(pos) else {
            return;
        };
        let current_track = if self.cursor == Some(pos) {
            None
        } else {
            self.current().cloned()
        };
        let mut flat: Vec<TrackId> = order.iter().map(|&i| self.items[i].clone()).collect();
        flat.remove(pos);
        self.items = flat;
        self.shuffle = None;
        let _ = victim_idx;
        if self.items.is_empty() {
            self.cursor = None;
        } else if let Some(track) = current_track {
            self.cursor = self.items.iter().position(|x| *x == track).or(Some(0));
        } else {
            // We removed the current track: keep the same slot, clamped.
            self.cursor = Some(pos.min(self.items.len() - 1));
        }
    }

    /// Turn shuffle on with a caller-supplied seed (deterministic), or off.
    ///
    /// Turning shuffle on builds a fresh permutation and re-anchors the cursor
    /// so the *currently-playing* track stays current (its new play position is
    /// found in the permutation). Turning it off restores stored order, again
    /// keeping the same track current.
    pub fn set_shuffle(&mut self, on: bool, seed: u64) {
        if self.items.is_empty() {
            self.shuffle = if on { Some(Vec::new()) } else { None };
            return;
        }
        let current_item_idx = self
            .cursor
            .and_then(|pos| self.play_order().get(pos).copied());
        if on {
            let order = shuffled_order(self.items.len(), seed);
            self.cursor = current_item_idx
                .and_then(|idx| order.iter().position(|&i| i == idx))
                .or(Some(0));
            self.shuffle = Some(order);
        } else {
            self.shuffle = None;
            // In stored order, the play position equals the item index.
            self.cursor = current_item_idx.or(Some(0));
        }
    }

    /// What would play if the current song ends, given the repeat mode.
    ///
    /// Returns the *play-order position* of the next track, or `None` if
    /// playback would stop (end of queue with repeat off).
    #[must_use]
    pub fn next_index(&self) -> Option<usize> {
        let pos = self.cursor?;
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        match self.repeat {
            RepeatMode::One => Some(pos),
            RepeatMode::Off => {
                if pos < last {
                    Some(pos + 1)
                } else {
                    None
                }
            }
            RepeatMode::All => {
                if pos < last {
                    Some(pos + 1)
                } else {
                    Some(0)
                }
            }
        }
    }

    /// What would play on "previous", given the repeat mode.
    ///
    /// Mirror of [`Queue::next_index`]: `One` stays put, `Off` stops at the
    /// start, `All` wraps to the end.
    #[must_use]
    pub fn previous_index(&self) -> Option<usize> {
        let pos = self.cursor?;
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        match self.repeat {
            RepeatMode::One => Some(pos),
            RepeatMode::Off => {
                if pos > 0 {
                    Some(pos - 1)
                } else {
                    None
                }
            }
            RepeatMode::All => {
                if pos > 0 {
                    Some(pos - 1)
                } else {
                    Some(last)
                }
            }
        }
    }

    /// Advance the cursor to the next track and return its id, if any.
    pub fn advance(&mut self) -> Option<&TrackId> {
        self.cursor = self.next_index();
        self.current()
    }

    /// Step the cursor back to the previous track and return its id, if any.
    pub fn go_back(&mut self) -> Option<&TrackId> {
        self.cursor = self.previous_index();
        self.current()
    }

    /// Jump the cursor directly to a play-order position. Out-of-range is
    /// ignored.
    pub fn select(&mut self, pos: usize) {
        if pos < self.items.len() {
            self.cursor = Some(pos);
        }
    }

    /// The track ids in current play order (what a UI would list).
    #[must_use]
    pub fn ordered_ids(&self) -> Vec<TrackId> {
        self.play_order()
            .iter()
            .filter_map(|&i| self.items.get(i).cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(xs: &[&str]) -> Vec<TrackId> {
        xs.iter().map(|s| TrackId::new(*s)).collect()
    }

    fn q(xs: &[&str]) -> Queue {
        let mut q = Queue::new();
        q.enqueue(ids(xs));
        q
    }

    #[test]
    fn fresh_queue_is_empty() {
        let q = Queue::new();
        assert!(q.is_empty());
        assert_eq!(q.current(), None);
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn enqueue_appends_and_selects_first() {
        let q = q(&["a", "b", "c"]);
        assert_eq!(q.len(), 3);
        assert_eq!(q.current(), Some(&TrackId::new("a")));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn enqueue_into_nonempty_keeps_current() {
        let mut q = q(&["a", "b"]);
        q.advance(); // now on "b"
        q.enqueue(ids(&["c", "d"]));
        assert_eq!(q.current(), Some(&TrackId::new("b")));
        assert_eq!(q.len(), 4);
    }

    #[test]
    fn enqueue_next_inserts_after_current() {
        let mut q = q(&["a", "b", "c"]);
        // current is "a"
        q.enqueue_next(ids(&["x"]));
        assert_eq!(q.ordered_ids(), ids(&["a", "x", "b", "c"]));
        assert_eq!(q.current(), Some(&TrackId::new("a")));
    }

    #[test]
    fn enqueue_next_on_empty_is_enqueue() {
        let mut q = Queue::new();
        q.enqueue_next(ids(&["a", "b"]));
        assert_eq!(q.ordered_ids(), ids(&["a", "b"]));
        assert_eq!(q.current(), Some(&TrackId::new("a")));
    }

    #[test]
    fn play_now_replaces_everything() {
        let mut q = q(&["a", "b", "c"]);
        q.advance();
        q.play_now(ids(&["x", "y"]));
        assert_eq!(q.ordered_ids(), ids(&["x", "y"]));
        assert_eq!(q.current(), Some(&TrackId::new("x")));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn clear_empties_and_unsets_cursor() {
        let mut q = q(&["a", "b"]);
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.current(), None);
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn move_item_reorders_and_keeps_current_track() {
        let mut q = q(&["a", "b", "c", "d"]);
        q.advance(); // current "b"
        q.move_item(0, 3); // move "a" to the end
        assert_eq!(q.ordered_ids(), ids(&["b", "c", "d", "a"]));
        // Still playing "b".
        assert_eq!(q.current(), Some(&TrackId::new("b")));
    }

    #[test]
    fn move_item_out_of_range_is_noop() {
        let mut q = q(&["a", "b"]);
        q.move_item(0, 9);
        assert_eq!(q.ordered_ids(), ids(&["a", "b"]));
    }

    #[test]
    fn remove_non_current_keeps_current() {
        let mut q = q(&["a", "b", "c"]);
        q.advance(); // current "b"
        q.remove_item(2); // remove "c"
        assert_eq!(q.ordered_ids(), ids(&["a", "b"]));
        assert_eq!(q.current(), Some(&TrackId::new("b")));
    }

    #[test]
    fn remove_current_advances_to_next_slot() {
        let mut q = q(&["a", "b", "c"]);
        q.advance(); // current "b"
        q.remove_item(1); // remove the current track
        assert_eq!(q.ordered_ids(), ids(&["a", "c"]));
        // Slot 1 now holds "c".
        assert_eq!(q.current(), Some(&TrackId::new("c")));
    }

    #[test]
    fn remove_last_remaining_clears_cursor() {
        let mut q = q(&["only"]);
        q.remove_item(0);
        assert!(q.is_empty());
        assert_eq!(q.current(), None);
    }

    #[test]
    fn next_off_stops_at_end() {
        let mut q = q(&["a", "b"]);
        q.set_repeat(RepeatMode::Off);
        assert_eq!(q.next_index(), Some(1));
        q.advance(); // on "b"
        assert_eq!(q.next_index(), None);
        assert_eq!(q.advance(), None);
    }

    #[test]
    fn next_one_repeats_current() {
        let mut q = q(&["a", "b"]);
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.next_index(), Some(0));
        q.advance();
        assert_eq!(q.current(), Some(&TrackId::new("a")));
    }

    #[test]
    fn next_all_wraps_to_start() {
        let mut q = q(&["a", "b"]);
        q.set_repeat(RepeatMode::All);
        q.select(1); // on "b" (last)
        assert_eq!(q.next_index(), Some(0));
        q.advance();
        assert_eq!(q.current(), Some(&TrackId::new("a")));
    }

    #[test]
    fn previous_off_stops_at_start() {
        let mut q = q(&["a", "b"]);
        q.set_repeat(RepeatMode::Off);
        assert_eq!(q.previous_index(), None);
    }

    #[test]
    fn previous_all_wraps_to_end() {
        let mut q = q(&["a", "b", "c"]);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.previous_index(), Some(2));
        q.go_back();
        assert_eq!(q.current(), Some(&TrackId::new("c")));
    }

    #[test]
    fn previous_one_stays_put() {
        let mut q = q(&["a", "b"]);
        q.advance(); // on "b"
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.previous_index(), Some(1));
    }

    #[test]
    fn shuffle_is_deterministic_for_a_seed() {
        let mut q1 = q(&["a", "b", "c", "d", "e"]);
        let mut q2 = q(&["a", "b", "c", "d", "e"]);
        q1.set_shuffle(true, 777);
        q2.set_shuffle(true, 777);
        assert_eq!(q1.ordered_ids(), q2.ordered_ids());
        assert!(q1.is_shuffled());
    }

    #[test]
    fn shuffle_covers_every_track_once() {
        let mut q = q(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        q.set_shuffle(true, 9);
        let mut got = q.ordered_ids();
        got.sort();
        assert_eq!(got, ids(&["a", "b", "c", "d", "e", "f", "g", "h"]));
    }

    #[test]
    fn shuffle_keeps_current_track_current() {
        let mut q = q(&["a", "b", "c", "d"]);
        q.select(2); // playing "c"
        q.set_shuffle(true, 3);
        assert_eq!(q.current(), Some(&TrackId::new("c")));
        q.set_shuffle(false, 0);
        // Back in stored order, still on "c" at its original index 2.
        assert_eq!(q.current(), Some(&TrackId::new("c")));
        assert_eq!(q.current_index(), Some(2));
    }

    #[test]
    fn enqueue_while_shuffled_adds_track() {
        let mut q = q(&["a", "b", "c"]);
        q.set_shuffle(true, 5);
        q.enqueue(ids(&["z"]));
        let mut got = q.ordered_ids();
        got.sort();
        assert_eq!(got, ids(&["a", "b", "c", "z"]));
    }

    #[test]
    fn ordered_ids_reflects_stored_order_unshuffled() {
        let q = q(&["a", "b", "c"]);
        assert_eq!(q.ordered_ids(), ids(&["a", "b", "c"]));
    }
}
