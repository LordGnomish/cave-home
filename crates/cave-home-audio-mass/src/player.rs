//! Per-room player state and the multi-room group model (ADR-020).
//!
//! A [`Player`] is one room's playback: its [`PlayState`] (playing / paused /
//! idle), its own [`Queue`], how far into the current song it is, its volume,
//! the crossfade/gapless preferences, and the "an announcement just interrupted
//! the music" save/restore. Each room owns its own player, so the kitchen and
//! the living room are independent — until they are joined into a [`PlayerGroup`]
//! to play the *same* queue in sync.
//!
//! ## The Snapcast boundary
//!
//! This module models *what* should play *where*; it does not make sound and it
//! does not synchronise clocks. Turning a [`PlayerGroup`] into actually-in-sync
//! audio across rooms is **Snapcast's** job (ADR-020: `cave-home-audio-snapcast`,
//! a clean-room port of the Snapcast wire protocol). This engine hands Snapcast
//! a resolved per-player queue and stays out of the real-time path. That hand-off
//! is deferred to Phase 1b (see the parity manifest).

use crate::media::TrackId;
use crate::queue::Queue;

/// A volume level, 0..=100, as a delegated value object (Charter §6.3: a person
/// thinks "louder/quieter", not "dB" or "0.0..1.0 gain").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume(u8);

/// The loudest a player can be set to.
pub const MAX_VOLUME: u8 = 100;

impl Volume {
    /// A volume, clamped into 0..=100 so it can never be invalid.
    #[must_use]
    pub const fn new(level: u8) -> Self {
        Self(if level > MAX_VOLUME { MAX_VOLUME } else { level })
    }

    /// The 0..=100 level.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.0
    }

    /// A louder volume, saturating at the maximum.
    #[must_use]
    pub const fn louder(self, step: u8) -> Self {
        Self::new(self.0.saturating_add(step))
    }

    /// A quieter volume, saturating at zero.
    #[must_use]
    pub const fn quieter(self, step: u8) -> Self {
        Self::new(self.0.saturating_sub(step))
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::new(30)
    }
}

/// What a player is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    /// Sound is coming out.
    Playing,
    /// A song is loaded and held, ready to resume.
    Paused,
    /// Nothing is playing and nothing is held.
    Idle,
}

/// Crossfade / gapless preferences for transitions between songs.
///
/// These are *intent* flags the playback pipeline (Phase 1b) reads; the engine
/// only stores and reports them. Crossfade and gapless are mutually sensible —
/// gapless is "no silence between tracks", crossfade is "blend the tail of one
/// into the head of the next".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransitionPrefs {
    /// Blend the end of one track into the start of the next.
    pub crossfade: bool,
    /// Length of the crossfade, in whole seconds (ignored when `crossfade` is
    /// off).
    pub crossfade_secs: u8,
    /// Remove the silence between tracks for continuous albums.
    pub gapless: bool,
}

/// A music playback saved across an announcement interruption.
///
/// When a TTS announcement ("Dinner is ready", a doorbell chime) needs the
/// room, the engine snapshots whether music was playing and how far in, pauses,
/// and restores afterwards — so the song resumes exactly where it left off.
#[derive(Debug, Clone, PartialEq)]
struct SavedPlayback {
    was_playing: bool,
    elapsed_secs: u32,
    track: Option<TrackId>,
}

/// One room's player.
#[derive(Debug, Clone)]
pub struct Player {
    name: String,
    state: PlayState,
    queue: Queue,
    elapsed_secs: u32,
    volume: Volume,
    transition: TransitionPrefs,
    /// `Some` while a TTS announcement is interrupting the music.
    saved: Option<SavedPlayback>,
}

impl Player {
    /// A new, idle player for a named room at the default volume.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            state: PlayState::Idle,
            queue: Queue::new(),
            elapsed_secs: 0,
            volume: Volume::default(),
            transition: TransitionPrefs::default(),
            saved: None,
        }
    }

    /// The room name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The current play state.
    #[must_use]
    pub const fn state(&self) -> PlayState {
        self.state
    }

    /// Read-only access to the player's queue.
    #[must_use]
    pub const fn queue(&self) -> &Queue {
        &self.queue
    }

    /// Mutable access to the player's queue (to enqueue, reorder, etc.).
    pub const fn queue_mut(&mut self) -> &mut Queue {
        &mut self.queue
    }

    /// Seconds elapsed into the current track.
    #[must_use]
    pub const fn elapsed_secs(&self) -> u32 {
        self.elapsed_secs
    }

    /// The current volume.
    #[must_use]
    pub const fn volume(&self) -> Volume {
        self.volume
    }

    /// Set the volume (already clamped by [`Volume`]).
    pub const fn set_volume(&mut self, volume: Volume) {
        self.volume = volume;
    }

    /// The crossfade / gapless preferences.
    #[must_use]
    pub const fn transition(&self) -> TransitionPrefs {
        self.transition
    }

    /// Set the crossfade / gapless preferences.
    pub const fn set_transition(&mut self, prefs: TransitionPrefs) {
        self.transition = prefs;
    }

    /// The track id currently selected in the queue, if any.
    #[must_use]
    pub fn current_track(&self) -> Option<&TrackId> {
        self.queue.current()
    }

    /// Start (or resume) playing. From idle with a non-empty queue, starts the
    /// current track at 0; from paused, resumes where it was.
    pub fn play(&mut self) {
        if self.queue.current().is_some() {
            self.state = PlayState::Playing;
        }
    }

    /// Pause, holding the current position.
    pub fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Paused;
        }
    }

    /// Stop and forget the position (back to idle).
    pub const fn stop(&mut self) {
        self.state = PlayState::Idle;
        self.elapsed_secs = 0;
    }

    /// Advance playback time within the current track (the pipeline calls this
    /// as the clock moves; here it is pure state).
    pub fn tick(&mut self, secs: u32) {
        if self.state == PlayState::Playing {
            self.elapsed_secs = self.elapsed_secs.saturating_add(secs);
        }
    }

    /// Skip to the next track per the queue's repeat/shuffle rules. Resets the
    /// elapsed timer; goes idle if there is nothing next.
    pub fn next_track(&mut self) {
        let had_next = self.queue.advance().is_some();
        self.elapsed_secs = 0;
        if !had_next {
            self.state = PlayState::Idle;
        }
    }

    /// Step back to the previous track per the queue's rules. Resets the timer.
    pub fn previous_track(&mut self) {
        self.queue.go_back();
        self.elapsed_secs = 0;
    }

    /// Begin a TTS announcement: snapshot the music, pause it, drop the timer.
    ///
    /// Idempotent for nested announcements — a second `begin` while one is
    /// already active keeps the *original* saved state, so the music still
    /// resumes correctly after both finish.
    pub fn begin_announcement(&mut self) {
        if self.saved.is_none() {
            self.saved = Some(SavedPlayback {
                was_playing: self.state == PlayState::Playing,
                elapsed_secs: self.elapsed_secs,
                track: self.queue.current().cloned(),
            });
        }
        self.state = PlayState::Paused;
    }

    /// End a TTS announcement: restore the music to exactly where it was.
    ///
    /// If music was playing before the announcement, it resumes playing at the
    /// same elapsed time; if it was paused or idle, it stays that way.
    pub fn end_announcement(&mut self) {
        if let Some(saved) = self.saved.take() {
            self.elapsed_secs = saved.elapsed_secs;
            self.state = if saved.was_playing {
                PlayState::Playing
            } else {
                self.state
            };
        }
    }

    /// Whether an announcement is currently interrupting the music.
    #[must_use]
    pub const fn announcement_active(&self) -> bool {
        self.saved.is_some()
    }
}

/// A multi-room group: several players told to play the *same* queue together.
///
/// The group models the fan-out — it resolves one shared queue and the set of
/// member rooms. Making those rooms actually play in sync (sample-aligned, no
/// echo) is Snapcast's job (ADR-020); this type is the hand-off boundary, not
/// the synchroniser. The leader's queue is the source of truth.
#[derive(Debug, Clone)]
pub struct PlayerGroup {
    leader: String,
    members: Vec<String>,
}

impl PlayerGroup {
    /// Create a group led by one room.
    #[must_use]
    pub fn new(leader: impl Into<String>) -> Self {
        let leader = leader.into();
        Self {
            members: vec![leader.clone()],
            leader,
        }
    }

    /// The room whose queue the group follows.
    #[must_use]
    pub fn leader(&self) -> &str {
        &self.leader
    }

    /// The member rooms (including the leader), in join order.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }

    /// Add a room to the group (no-op if already a member).
    pub fn join(&mut self, room: impl Into<String>) {
        let room = room.into();
        if !self.members.contains(&room) {
            self.members.push(room);
        }
    }

    /// Remove a room from the group. Removing the leader is ignored (the leader
    /// must hand off first); removing the last follower is allowed.
    pub fn leave(&mut self, room: &str) {
        if room == self.leader {
            return;
        }
        self.members.retain(|m| m != room);
    }

    /// How many rooms are in the group.
    #[must_use]
    pub fn size(&self) -> usize {
        self.members.len()
    }

    /// Fan a leader's queue out to every member: the resolved per-player queue
    /// each room should load. This is the data Snapcast (Phase 1b) consumes; the
    /// engine produces it, it does not stream it.
    #[must_use]
    pub fn fan_out(&self, leader_queue: &Queue) -> Vec<(String, Vec<TrackId>)> {
        let shared = leader_queue.ordered_ids();
        self.members
            .iter()
            .map(|room| (room.clone(), shared.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::TrackId;

    fn ids(xs: &[&str]) -> Vec<TrackId> {
        xs.iter().map(|s| TrackId::new(*s)).collect()
    }

    #[test]
    fn volume_clamps_and_steps() {
        assert_eq!(Volume::new(250).level(), 100);
        assert_eq!(Volume::new(20).louder(15).level(), 35);
        assert_eq!(Volume::new(10).quieter(50).level(), 0);
        assert_eq!(Volume::new(95).louder(20).level(), 100);
    }

    #[test]
    fn new_player_is_idle() {
        let p = Player::new("Kitchen");
        assert_eq!(p.state(), PlayState::Idle);
        assert_eq!(p.name(), "Kitchen");
        assert!(p.current_track().is_none());
    }

    #[test]
    fn play_pause_resume_cycle() {
        let mut p = Player::new("Living room");
        p.queue_mut().enqueue(ids(&["a", "b"]));
        p.play();
        assert_eq!(p.state(), PlayState::Playing);
        p.tick(30);
        assert_eq!(p.elapsed_secs(), 30);
        p.pause();
        assert_eq!(p.state(), PlayState::Paused);
        p.tick(10); // no advance while paused
        assert_eq!(p.elapsed_secs(), 30);
        p.play();
        assert_eq!(p.state(), PlayState::Playing);
    }

    #[test]
    fn next_track_resets_timer() {
        let mut p = Player::new("Den");
        p.queue_mut().enqueue(ids(&["a", "b"]));
        p.play();
        p.tick(40);
        p.next_track();
        assert_eq!(p.elapsed_secs(), 0);
        assert_eq!(p.current_track(), Some(&TrackId::new("b")));
    }

    #[test]
    fn next_off_end_goes_idle() {
        let mut p = Player::new("Den");
        p.queue_mut().enqueue(ids(&["only"]));
        p.play();
        p.next_track(); // nothing after "only" with repeat off
        assert_eq!(p.state(), PlayState::Idle);
    }

    #[test]
    fn announcement_saves_and_restores_playing_music() {
        let mut p = Player::new("Kitchen");
        p.queue_mut().enqueue(ids(&["song"]));
        p.play();
        p.tick(45);
        p.begin_announcement();
        assert!(p.announcement_active());
        assert_eq!(p.state(), PlayState::Paused);
        p.end_announcement();
        assert!(!p.announcement_active());
        assert_eq!(p.state(), PlayState::Playing, "music that was playing resumes");
        assert_eq!(p.elapsed_secs(), 45, "resumes at the same position");
    }

    #[test]
    fn announcement_does_not_start_paused_music() {
        let mut p = Player::new("Kitchen");
        p.queue_mut().enqueue(ids(&["song"]));
        p.play();
        p.pause();
        p.begin_announcement();
        p.end_announcement();
        // Was paused before the announcement -> stays paused, does not play.
        assert_eq!(p.state(), PlayState::Paused);
    }

    #[test]
    fn nested_announcement_preserves_original_save() {
        let mut p = Player::new("Kitchen");
        p.queue_mut().enqueue(ids(&["song"]));
        p.play();
        p.tick(20);
        p.begin_announcement(); // doorbell
        p.begin_announcement(); // a second chime while the first is up
        p.end_announcement();
        assert_eq!(p.state(), PlayState::Playing);
        assert_eq!(p.elapsed_secs(), 20);
    }

    #[test]
    fn transition_prefs_round_trip() {
        let mut p = Player::new("Office");
        let prefs = TransitionPrefs { crossfade: true, crossfade_secs: 5, gapless: false };
        p.set_transition(prefs);
        assert_eq!(p.transition(), prefs);
    }

    #[test]
    fn players_are_independent() {
        let mut kitchen = Player::new("Kitchen");
        let mut living = Player::new("Living room");
        kitchen.queue_mut().enqueue(ids(&["k1", "k2"]));
        living.queue_mut().enqueue(ids(&["l1"]));
        kitchen.play();
        // Living room is untouched by the kitchen.
        assert_eq!(living.state(), PlayState::Idle);
        assert_eq!(living.current_track(), Some(&TrackId::new("l1")));
        assert_eq!(kitchen.current_track(), Some(&TrackId::new("k1")));
    }

    #[test]
    fn group_join_and_fan_out() {
        let mut group = PlayerGroup::new("Kitchen");
        group.join("Living room");
        group.join("Kitchen"); // duplicate ignored
        assert_eq!(group.size(), 2);
        assert_eq!(group.leader(), "Kitchen");

        let mut leader_queue = Queue::new();
        leader_queue.enqueue(ids(&["a", "b"]));
        let plan = group.fan_out(&leader_queue);
        assert_eq!(plan.len(), 2);
        for (_room, q) in &plan {
            assert_eq!(*q, ids(&["a", "b"]), "every room gets the same shared queue");
        }
    }

    #[test]
    fn group_leave_drops_follower_not_leader() {
        let mut group = PlayerGroup::new("Kitchen");
        group.join("Living room");
        group.leave("Living room");
        assert_eq!(group.size(), 1);
        group.leave("Kitchen"); // leader can't leave its own group
        assert_eq!(group.size(), 1);
    }
}
