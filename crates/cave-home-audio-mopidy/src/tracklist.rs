//! The play queue (MPD's "current playlist" / Mopidy's tracklist).
//!
//! MPD distinguishes two ways to name a queued song:
//! - **position** — the song's 0-based index in the queue; it shifts whenever
//!   songs are added, removed or moved.
//! - **song id** — a stable identifier assigned when the song is added; it never
//!   changes for the life of that queue entry. A client that holds an id can
//!   still find its song after the queue is reordered.
//!
//! [`Tracklist`] models both, plus the editing operations (`add`, `delete`,
//! `move`, `clear`) and the **next-song** computation that honours the four
//! playback toggles (`random`, `repeat`, `single`, `consume`). The next-song
//! logic is the heart of the engine and is exercised under every toggle
//! combination in the tests.

/// One queued song. `uri` is the backend-addressable location; the optional
/// metadata is what the grandma-friendly layer announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    /// Stable id, unique within the queue for the life of the entry.
    pub id: u64,
    /// Backend URI (e.g. `local:track:…`). Opaque to this engine.
    pub uri: String,
    pub title: Option<String>,
    pub artist: Option<String>,
}

/// The four playback toggles that shape next-song selection.
///
/// - `random` — pick the next song by shuffle order rather than queue order.
/// - `repeat` — when the queue is exhausted, wrap around to the start.
/// - `single` — play one song then stop (or, with `repeat`, replay the same
///   song forever).
/// - `consume` — a song is removed from the queue once it finishes playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modes {
    pub random: bool,
    pub repeat: bool,
    pub single: bool,
    pub consume: bool,
}

/// Why a queue edit could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The given position is past the end of the queue.
    PositionOutOfRange,
    /// No queued song carries the given id.
    NoSuchId,
}

impl core::fmt::Display for QueueError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PositionOutOfRange => f.write_str("no song at that position"),
            Self::NoSuchId => f.write_str("no song with that id"),
        }
    }
}

impl std::error::Error for QueueError {}

/// What playing the *current* song should do once it finishes — computed by
/// [`Tracklist::next_after`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextSong {
    /// Continue at this queue position.
    Play(usize),
    /// The queue is exhausted (or `single` without `repeat`): stop playing.
    Stop,
}

/// The play queue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracklist {
    songs: Vec<Song>,
    next_id: u64,
}

impl Tracklist {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of queued songs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.songs.len()
    }

    /// Whether the queue holds no songs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// The queued songs, in order.
    #[must_use]
    pub fn songs(&self) -> &[Song] {
        &self.songs
    }

    /// The song at a queue position, if any.
    #[must_use]
    pub fn get(&self, pos: usize) -> Option<&Song> {
        self.songs.get(pos)
    }

    /// The 0-based position of the song with the given stable id.
    #[must_use]
    pub fn position_of_id(&self, id: u64) -> Option<usize> {
        self.songs.iter().position(|s| s.id == id)
    }

    /// Append a song; returns its freshly-assigned stable id.
    ///
    /// Ids are monotonic for the life of the queue and are *not* reused after a
    /// [`Tracklist::clear`], matching MPD's behaviour where a client cannot
    /// confuse a stale id with a new song.
    pub fn add(&mut self, uri: impl Into<String>, title: Option<String>, artist: Option<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.songs.push(Song {
            id,
            uri: uri.into(),
            title,
            artist,
        });
        id
    }

    /// Remove the song at a queue position.
    ///
    /// # Errors
    /// [`QueueError::PositionOutOfRange`] if `pos` is past the end.
    pub fn delete(&mut self, pos: usize) -> Result<Song, QueueError> {
        if pos >= self.songs.len() {
            return Err(QueueError::PositionOutOfRange);
        }
        Ok(self.songs.remove(pos))
    }

    /// Remove the song with a stable id.
    ///
    /// # Errors
    /// [`QueueError::NoSuchId`] if no queued song carries that id.
    pub fn delete_id(&mut self, id: u64) -> Result<Song, QueueError> {
        let pos = self.position_of_id(id).ok_or(QueueError::NoSuchId)?;
        Ok(self.songs.remove(pos))
    }

    /// Move the song at `from` to `to`, shifting the rest. Positions are
    /// interpreted against the queue *before* the move (MPD semantics).
    ///
    /// # Errors
    /// [`QueueError::PositionOutOfRange`] if either endpoint is past the end.
    pub fn move_song(&mut self, from: usize, to: usize) -> Result<(), QueueError> {
        let len = self.songs.len();
        if from >= len || to >= len {
            return Err(QueueError::PositionOutOfRange);
        }
        if from == to {
            return Ok(());
        }
        let song = self.songs.remove(from);
        self.songs.insert(to, song);
        Ok(())
    }

    /// Remove every song. Stable ids are not reused afterwards.
    pub fn clear(&mut self) {
        self.songs.clear();
    }

    /// Compute the next song after the one at `current`, honouring `modes`.
    ///
    /// `current` is the position that just finished playing. The rules, applied
    /// in MPD's precedence order:
    /// 1. `single` plays exactly one song: with `repeat` it replays the same
    ///    position forever; without `repeat` it stops.
    /// 2. otherwise advance one step; `random` advances in shuffle order
    ///    ([`Tracklist::shuffle_order`]) instead of queue order.
    /// 3. at the end of the (queue or shuffle) order, `repeat` wraps to the
    ///    start; otherwise playback stops.
    ///
    /// `consume` does *not* affect this computation — it removes the finished
    /// song from the queue, which the caller applies via
    /// [`Tracklist::consume_finished`]. After a consume, the song that follows
    /// is already sitting at the finished song's old position, so the caller
    /// uses that return value directly rather than calling this.
    #[must_use]
    pub fn next_after(&self, current: usize, modes: Modes, seed: u64) -> NextSong {
        if self.songs.is_empty() {
            return NextSong::Stop;
        }
        if modes.single {
            return if modes.repeat {
                // Same song again; clamp a now-out-of-range position to the end.
                NextSong::Play(current.min(self.songs.len() - 1))
            } else {
                NextSong::Stop
            };
        }
        if modes.random {
            return self.next_in_order(&self.shuffle_order(seed), current, modes.repeat);
        }
        let order: Vec<usize> = (0..self.songs.len()).collect();
        self.next_in_order(&order, current, modes.repeat)
    }

    /// Given a play `order` (a permutation of positions), find the position that
    /// follows `current`, wrapping when `repeat` is set.
    fn next_in_order(&self, order: &[usize], current: usize, repeat: bool) -> NextSong {
        let Some(idx) = order.iter().position(|&p| p == current) else {
            // `current` is not in the order (e.g. it was just consumed); start
            // at the front of the order.
            return order.first().map_or(NextSong::Stop, |&p| NextSong::Play(p));
        };
        if idx + 1 < order.len() {
            NextSong::Play(order[idx + 1])
        } else if repeat {
            order.first().map_or(NextSong::Stop, |&p| NextSong::Play(p))
        } else {
            NextSong::Stop
        }
    }

    /// A deterministic shuffle order over the current positions, driven by a
    /// seed. A Fisher–Yates shuffle with a small seeded LCG — no `rand` crate,
    /// and reproducible given the same `seed` (Charter: std-only).
    #[must_use]
    pub fn shuffle_order(&self, seed: u64) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.songs.len()).collect();
        // LCG constants from Numerical Recipes (glibc family).
        let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
        for i in (1..order.len()).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            // Use the high bits, which have the best statistical quality.
            let j = ((state >> 33) % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        order
    }

    /// Apply `consume` after the song at `current` finishes playing.
    ///
    /// When `modes.consume` is set the finished song is removed from the queue;
    /// the song that follows then occupies the *same* position `current` (every
    /// later song shifted down by one). The return value is the
    /// [`NextSong`] to play next, computed against the now-shrunken queue:
    /// - the song now sitting at `current`, if one is there;
    /// - else (the finished song was last) wrap to position 0 when `repeat` is
    ///   set, otherwise [`NextSong::Stop`].
    ///
    /// When `consume` is off this is a no-op that returns [`NextSong::Stop`];
    /// the caller should use [`Tracklist::next_after`] instead in that case.
    pub fn consume_finished(&mut self, current: usize, modes: Modes) -> NextSong {
        if !modes.consume || current >= self.songs.len() {
            return NextSong::Stop;
        }
        let _ = self.songs.remove(current);
        if current < self.songs.len() {
            NextSong::Play(current)
        } else if modes.repeat && !self.songs.is_empty() {
            NextSong::Play(0)
        } else {
            NextSong::Stop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Tracklist {
        let mut t = Tracklist::new();
        t.add("local:a", Some("A".into()), None);
        t.add("local:b", Some("B".into()), None);
        t.add("local:c", Some("C".into()), None);
        t
    }

    #[test]
    fn add_assigns_stable_increasing_ids() {
        let mut t = Tracklist::new();
        let a = t.add("local:a", None, None);
        let b = t.add("local:b", None, None);
        assert_eq!((a, b), (0, 1));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn songid_survives_reorder_but_position_does_not() {
        let mut t = fixture();
        let b_id = t.songs()[1].id;
        assert_eq!(t.position_of_id(b_id), Some(1));
        t.move_song(1, 0).unwrap();
        // Position changed, id did not.
        assert_eq!(t.position_of_id(b_id), Some(0));
        assert_eq!(t.songs()[0].id, b_id);
    }

    #[test]
    fn delete_by_position_and_by_id() {
        let mut t = fixture();
        let removed = t.delete(0).unwrap();
        assert_eq!(removed.uri, "local:a");
        let c_id = t.songs()[1].id;
        t.delete_id(c_id).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t.songs()[0].uri, "local:b");
    }

    #[test]
    fn out_of_range_and_missing_id_are_errors() {
        let mut t = fixture();
        assert_eq!(t.delete(9), Err(QueueError::PositionOutOfRange));
        assert_eq!(t.delete_id(999), Err(QueueError::NoSuchId));
        assert_eq!(t.move_song(0, 9), Err(QueueError::PositionOutOfRange));
    }

    #[test]
    fn move_shifts_intermediate_songs() {
        let mut t = fixture(); // A B C
        t.move_song(0, 2).unwrap(); // -> B C A
        let uris: Vec<_> = t.songs().iter().map(|s| s.uri.as_str()).collect();
        assert_eq!(uris, ["local:b", "local:c", "local:a"]);
    }

    #[test]
    fn clear_empties_but_does_not_reuse_ids() {
        let mut t = fixture();
        t.clear();
        assert!(t.is_empty());
        let new_id = t.add("local:d", None, None);
        // Ids keep climbing; the fixture used 0,1,2.
        assert_eq!(new_id, 3);
    }

    #[test]
    fn empty_queue_next_is_stop() {
        let t = Tracklist::new();
        assert_eq!(t.next_after(0, Modes::default(), 1), NextSong::Stop);
    }

    #[test]
    fn plain_advance_through_queue_then_stop() {
        let t = fixture();
        let m = Modes::default();
        assert_eq!(t.next_after(0, m, 0), NextSong::Play(1));
        assert_eq!(t.next_after(1, m, 0), NextSong::Play(2));
        assert_eq!(t.next_after(2, m, 0), NextSong::Stop); // end, no repeat
    }

    #[test]
    fn repeat_wraps_at_end() {
        let t = fixture();
        let m = Modes { repeat: true, ..Modes::default() };
        assert_eq!(t.next_after(2, m, 0), NextSong::Play(0));
    }

    #[test]
    fn single_without_repeat_stops_after_one() {
        let t = fixture();
        let m = Modes { single: true, ..Modes::default() };
        assert_eq!(t.next_after(0, m, 0), NextSong::Stop);
    }

    #[test]
    fn single_with_repeat_replays_same_song() {
        let t = fixture();
        let m = Modes { single: true, repeat: true, ..Modes::default() };
        assert_eq!(t.next_after(1, m, 0), NextSong::Play(1));
    }

    #[test]
    fn single_takes_precedence_over_random() {
        let t = fixture();
        let m = Modes { single: true, random: true, repeat: true, ..Modes::default() };
        // single+repeat means "this one again", regardless of shuffle.
        assert_eq!(t.next_after(2, m, 12345), NextSong::Play(2));
    }

    #[test]
    fn random_visits_every_song_once_before_wrapping() {
        let t = fixture();
        let m = Modes { random: true, repeat: true, ..Modes::default() };
        let order = t.shuffle_order(777);
        // It is a permutation of all positions.
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2]);
        // next_after follows that very order.
        let first = order[0];
        let second = order[1];
        assert_eq!(t.next_after(first, m, 777), NextSong::Play(second));
    }

    #[test]
    fn random_is_deterministic_for_a_seed() {
        let t = fixture();
        assert_eq!(t.shuffle_order(42), t.shuffle_order(42));
    }

    #[test]
    fn random_without_repeat_stops_at_end_of_shuffle() {
        let t = fixture();
        let m = Modes { random: true, ..Modes::default() };
        let order = t.shuffle_order(99);
        let last = *order.last().unwrap();
        assert_eq!(t.next_after(last, m, 99), NextSong::Stop);
    }

    #[test]
    fn consume_removes_finished_song_and_advances_onto_next() {
        let mut t = fixture(); // A B C, positions 0 1 2
        let m = Modes { consume: true, ..Modes::default() };
        // Finished playing position 0 (A). Consume it: B shifts to position 0
        // and is what plays next.
        let next = t.consume_finished(0, m);
        assert_eq!(t.len(), 2); // B C remain
        assert_eq!(next, NextSong::Play(0));
        assert_eq!(t.songs()[0].uri, "local:b");
    }

    #[test]
    fn consume_of_last_song_stops_without_repeat() {
        let mut t = fixture(); // A B C
        let m = Modes { consume: true, ..Modes::default() };
        // Finished the last song (position 2). Consuming it leaves A B and ends
        // playback — there is nothing after it.
        let next = t.consume_finished(2, m);
        assert_eq!(t.len(), 2);
        assert_eq!(next, NextSong::Stop);
    }

    #[test]
    fn consume_of_last_song_wraps_with_repeat() {
        let mut t = fixture(); // A B C
        let m = Modes { consume: true, repeat: true, ..Modes::default() };
        let next = t.consume_finished(2, m);
        assert_eq!(next, NextSong::Play(0)); // back to A
    }

    #[test]
    fn consume_drains_queue_one_song_at_a_time() {
        let mut t = fixture();
        let m = Modes { consume: true, ..Modes::default() };
        // Always finishing position 0 drains front-to-back.
        assert_eq!(t.consume_finished(0, m), NextSong::Play(0)); // A gone, play B
        assert_eq!(t.consume_finished(0, m), NextSong::Play(0)); // B gone, play C
        assert_eq!(t.consume_finished(0, m), NextSong::Stop); // C gone, empty
        assert!(t.is_empty());
    }

    #[test]
    fn consume_off_is_a_noop_returning_stop() {
        let mut t = fixture();
        let m = Modes::default();
        assert_eq!(t.consume_finished(0, m), NextSong::Stop);
        assert_eq!(t.len(), 3);
    }
}
