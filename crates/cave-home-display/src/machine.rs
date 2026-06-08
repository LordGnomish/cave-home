//! The display control state machine: apply a [`MediaCommand`], gated by power,
//! playback rules, the source/app catalog, and the volume cap.
//!
//! Modelled on the Home Assistant `media_player` service surface (Apache-2.0).
//! The rules are deliberately conservative and household-legible:
//!
//! - While the TV is [`PowerState::Off`] or [`PowerState::Standby`], the only
//!   accepted command is [`MediaCommand::PowerOn`]; everything else is rejected
//!   with [`CommandError::PoweredOff`]. (Power-off is idempotently accepted.)
//! - Transport verbs (play / pause / stop / next / previous / seek) require the
//!   TV to be on *and* to have loaded content; next/previous/seek on an idle
//!   home screen are [`CommandError::NothingPlaying`].
//! - [`MediaCommand::SelectSource`] for an input the TV does not have is
//!   [`CommandError::UnknownSource`]; [`MediaCommand::LaunchApp`] for an app that
//!   is not installed is [`CommandError::UnknownApp`], and on a non-smart TV is
//!   [`CommandError::AppsUnsupported`].
//! - [`MediaCommand::SetVolume`] clamps to the parental cap rather than failing.

use crate::playback::PlaybackState;
use crate::power::PowerState;
use crate::source::{AppCapability, SourceCatalog};
use crate::volume::Volume;

/// A control verb the household (or an automation, or a voice command) issues to
/// a display. Mirrors the Home Assistant `media_player` service set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaCommand {
    /// Turn the TV on (wakes from off or standby).
    PowerOn,
    /// Turn the TV off.
    PowerOff,
    /// Play / resume.
    Play,
    /// Pause.
    Pause,
    /// Stop.
    Stop,
    /// Skip to the next item.
    Next,
    /// Go to the previous item.
    Previous,
    /// Set the sound level to a percentage (clamped to `0..=cap`).
    SetVolume(u8),
    /// Mute (`true`) or unmute (`false`).
    SetMute(bool),
    /// Switch to an input by its id (e.g. `"hdmi1"`).
    SelectSource(String),
    /// Launch an installed app by its id (e.g. `"netflix"`).
    LaunchApp(String),
    /// Seek forwards (positive) or backwards (negative) by some seconds,
    /// relative to the current position.
    SeekRelative(i64),
}

/// Why the machine rejected a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The TV is off or in standby; only powering on is accepted.
    PoweredOff,
    /// A transport verb was issued but nothing is loaded to control.
    NothingPlaying,
    /// The requested input is not one this TV has.
    UnknownSource,
    /// The requested app is not installed on this TV.
    UnknownApp,
    /// This TV cannot run apps at all.
    AppsUnsupported,
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PoweredOff => f.write_str("the TV is off"),
            Self::NothingPlaying => f.write_str("nothing is playing right now"),
            Self::UnknownSource => f.write_str("the TV does not have that input"),
            Self::UnknownApp => f.write_str("that app is not installed on the TV"),
            Self::AppsUnsupported => f.write_str("this TV cannot run apps"),
        }
    }
}

impl std::error::Error for CommandError {}

/// A display: its power state, playback state, sound, the input it is on, and the
/// catalog of inputs/apps it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    power: PowerState,
    playback: PlaybackState,
    volume: Volume,
    catalog: SourceCatalog,
    current_source: Option<String>,
    current_app: Option<String>,
    /// Current playback position in seconds (for relative seeking).
    position_secs: i64,
}

impl Display {
    /// A TV that is off, with the given input/app catalog and an initial volume.
    #[must_use]
    pub fn new(catalog: SourceCatalog, volume: Volume) -> Self {
        Self {
            power: PowerState::Off,
            playback: PlaybackState::Idle,
            volume,
            catalog,
            current_source: None,
            current_app: None,
            position_secs: 0,
        }
    }

    /// A TV that is already on and idle on its home screen.
    #[must_use]
    pub fn powered_on(catalog: SourceCatalog, volume: Volume) -> Self {
        let mut d = Self::new(catalog, volume);
        d.power = PowerState::On;
        d.playback = PlaybackState::Idle;
        d
    }

    /// The current power state.
    #[must_use]
    pub const fn power(&self) -> PowerState {
        self.power
    }

    /// The current playback state.
    #[must_use]
    pub const fn playback(&self) -> PlaybackState {
        self.playback
    }

    /// The current volume (level, mute and cap).
    #[must_use]
    pub const fn volume(&self) -> Volume {
        self.volume
    }

    /// The id of the input the TV is currently on, if any.
    #[must_use]
    pub fn current_source(&self) -> Option<&str> {
        self.current_source.as_deref()
    }

    /// The id of the app currently launched, if any.
    #[must_use]
    pub fn current_app(&self) -> Option<&str> {
        self.current_app.as_deref()
    }

    /// The current playback position in seconds.
    #[must_use]
    pub const fn position_secs(&self) -> i64 {
        self.position_secs
    }

    /// The input/app catalog this TV has.
    #[must_use]
    pub const fn catalog(&self) -> &SourceCatalog {
        &self.catalog
    }

    /// Whether a command would be accepted in the current state, without
    /// applying it. Pure query — never mutates.
    #[must_use]
    pub fn can_apply(&self, command: &MediaCommand) -> bool {
        self.check(command).is_ok()
    }

    /// Validate a command against the current state, returning the reason it
    /// would be rejected (if any). Does not mutate.
    fn check(&self, command: &MediaCommand) -> Result<(), CommandError> {
        // Power gating: while not active, only PowerOn / PowerOff pass.
        if !self.power.is_active() {
            return match command {
                MediaCommand::PowerOn | MediaCommand::PowerOff => Ok(()),
                _ => Err(CommandError::PoweredOff),
            };
        }

        match command {
            MediaCommand::PowerOn
            | MediaCommand::PowerOff
            | MediaCommand::Play
            | MediaCommand::Stop
            | MediaCommand::Pause
            | MediaCommand::SetVolume(_)
            | MediaCommand::SetMute(_) => Ok(()),

            MediaCommand::Next | MediaCommand::Previous | MediaCommand::SeekRelative(_) => {
                if self.playback.has_content() {
                    Ok(())
                } else {
                    Err(CommandError::NothingPlaying)
                }
            }

            MediaCommand::SelectSource(id) => {
                if self.catalog.find_source(id).is_some() {
                    Ok(())
                } else {
                    Err(CommandError::UnknownSource)
                }
            }

            MediaCommand::LaunchApp(id) => {
                if self.catalog.app_capability() == AppCapability::InputsOnly {
                    Err(CommandError::AppsUnsupported)
                } else if self.catalog.find_app(id).is_some() {
                    Ok(())
                } else {
                    Err(CommandError::UnknownApp)
                }
            }
        }
    }

    /// Apply a command, mutating the display, or reject it with the reason.
    ///
    /// # Errors
    ///
    /// Returns a [`CommandError`] when the command is not valid in the current
    /// state (powered off, nothing playing, unknown input/app, apps
    /// unsupported). On error the display is left unchanged.
    pub fn apply(&mut self, command: MediaCommand) -> Result<(), CommandError> {
        self.check(&command)?;

        match command {
            MediaCommand::PowerOn => {
                if !self.power.is_active() {
                    self.power = PowerState::On;
                    self.playback = PlaybackState::Idle;
                }
            }
            MediaCommand::PowerOff => {
                self.power = PowerState::Off;
                self.playback = PlaybackState::Idle;
                self.current_app = None;
                self.position_secs = 0;
            }
            MediaCommand::Play => self.playback = self.playback.play(),
            MediaCommand::Pause => self.playback = self.playback.pause(),
            MediaCommand::Stop => {
                self.playback = self.playback.stop();
                self.position_secs = 0;
            }
            MediaCommand::Next | MediaCommand::Previous => {
                // The track changes; position resets and playback continues.
                self.position_secs = 0;
                self.playback = self.playback.play();
            }
            MediaCommand::SetVolume(level) => self.volume = self.volume.set_level(level),
            MediaCommand::SetMute(muted) => self.volume = self.volume.set_mute(muted),
            MediaCommand::SelectSource(id) => {
                self.current_source = Some(id);
                self.current_app = None;
                self.playback = PlaybackState::Idle;
                self.position_secs = 0;
            }
            MediaCommand::LaunchApp(id) => {
                self.current_app = Some(id);
                self.playback = PlaybackState::Buffering;
                self.position_secs = 0;
            }
            MediaCommand::SeekRelative(delta) => {
                self.position_secs = self.position_secs.saturating_add(delta).max(0);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::App;

    fn on_tv() -> Display {
        Display::powered_on(SourceCatalog::typical_smart_tv(), Volume::new(20))
    }

    #[test]
    fn off_tv_rejects_everything_but_power() {
        let mut tv = Display::new(SourceCatalog::typical_smart_tv(), Volume::new(20));
        assert_eq!(tv.apply(MediaCommand::Play), Err(CommandError::PoweredOff));
        assert_eq!(
            tv.apply(MediaCommand::SetVolume(50)),
            Err(CommandError::PoweredOff)
        );
        assert_eq!(
            tv.apply(MediaCommand::SelectSource("hdmi1".into())),
            Err(CommandError::PoweredOff)
        );
        // Power-off on an off TV is accepted (idempotent).
        assert_eq!(tv.apply(MediaCommand::PowerOff), Ok(()));
        // Power-on wakes it.
        assert_eq!(tv.apply(MediaCommand::PowerOn), Ok(()));
        assert!(tv.power().is_on());
    }

    #[test]
    fn power_on_lands_idle() {
        let mut tv = Display::new(SourceCatalog::typical_smart_tv(), Volume::new(20));
        tv.apply(MediaCommand::PowerOn).unwrap();
        assert_eq!(tv.playback(), PlaybackState::Idle);
    }

    #[test]
    fn power_off_clears_app_and_position() {
        let mut tv = on_tv();
        tv.apply(MediaCommand::LaunchApp("netflix".into())).unwrap();
        tv.apply(MediaCommand::SeekRelative(30)).unwrap();
        tv.apply(MediaCommand::PowerOff).unwrap();
        assert_eq!(tv.power(), PowerState::Off);
        assert_eq!(tv.current_app(), None);
        assert_eq!(tv.position_secs(), 0);
    }

    #[test]
    fn standby_is_gated_like_off() {
        let mut tv = on_tv();
        // Force standby to exercise the gate.
        tv = Display {
            power: PowerState::Standby,
            ..tv
        };
        assert_eq!(tv.apply(MediaCommand::Play), Err(CommandError::PoweredOff));
        assert!(tv.can_apply(&MediaCommand::PowerOn));
    }

    #[test]
    fn play_pause_stop_transitions() {
        let mut tv = on_tv();
        tv.apply(MediaCommand::SelectSource("hdmi1".into())).unwrap();
        tv.apply(MediaCommand::Play).unwrap();
        assert_eq!(tv.playback(), PlaybackState::Playing);
        tv.apply(MediaCommand::Pause).unwrap();
        assert_eq!(tv.playback(), PlaybackState::Paused);
        tv.apply(MediaCommand::Play).unwrap();
        assert_eq!(tv.playback(), PlaybackState::Playing);
        tv.apply(MediaCommand::Stop).unwrap();
        assert_eq!(tv.playback(), PlaybackState::Stopped);
    }

    #[test]
    fn next_previous_need_content() {
        let mut tv = on_tv();
        // Idle home screen: nothing to skip.
        assert_eq!(tv.apply(MediaCommand::Next), Err(CommandError::NothingPlaying));
        assert_eq!(
            tv.apply(MediaCommand::SeekRelative(10)),
            Err(CommandError::NothingPlaying)
        );
        // Once content is loaded, skipping works.
        tv.apply(MediaCommand::LaunchApp("netflix".into())).unwrap();
        tv.apply(MediaCommand::Play).unwrap();
        assert_eq!(tv.apply(MediaCommand::Next), Ok(()));
        assert_eq!(tv.playback(), PlaybackState::Playing);
    }

    #[test]
    fn select_unknown_source_rejected() {
        let mut tv = on_tv();
        assert_eq!(
            tv.apply(MediaCommand::SelectSource("scart".into())),
            Err(CommandError::UnknownSource)
        );
        assert_eq!(tv.apply(MediaCommand::SelectSource("hdmi2".into())), Ok(()));
        assert_eq!(tv.current_source(), Some("hdmi2"));
    }

    #[test]
    fn launch_unknown_app_rejected() {
        let mut tv = on_tv();
        assert_eq!(
            tv.apply(MediaCommand::LaunchApp("disney".into())),
            Err(CommandError::UnknownApp)
        );
        assert_eq!(tv.apply(MediaCommand::LaunchApp("youtube".into())), Ok(()));
        assert_eq!(tv.current_app(), Some("youtube"));
    }

    #[test]
    fn non_smart_tv_cannot_launch_apps() {
        let catalog = SourceCatalog::new()
            .add_source(crate::source::Source::new("hdmi1", "HDMI 1"))
            .add_app(App::new("netflix", "Netflix"))
            .with_app_capability(AppCapability::InputsOnly);
        let mut tv = Display::powered_on(catalog, Volume::new(20));
        assert_eq!(
            tv.apply(MediaCommand::LaunchApp("netflix".into())),
            Err(CommandError::AppsUnsupported)
        );
    }

    #[test]
    fn set_volume_clamps_to_cap() {
        let mut tv = Display::powered_on(SourceCatalog::typical_smart_tv(), Volume::with_cap(10, 40));
        tv.apply(MediaCommand::SetVolume(95)).unwrap();
        assert_eq!(tv.volume().level(), 40, "parental cap holds");
    }

    #[test]
    fn mute_then_unmute_restores_level() {
        let mut tv = Display::powered_on(SourceCatalog::typical_smart_tv(), Volume::new(55));
        tv.apply(MediaCommand::SetMute(true)).unwrap();
        assert_eq!(tv.volume().audible_level(), 0);
        tv.apply(MediaCommand::SetMute(false)).unwrap();
        assert_eq!(tv.volume().audible_level(), 55);
    }

    #[test]
    fn selecting_a_source_clears_app_and_resets_playback() {
        let mut tv = on_tv();
        tv.apply(MediaCommand::LaunchApp("netflix".into())).unwrap();
        assert_eq!(tv.current_app(), Some("netflix"));
        tv.apply(MediaCommand::SelectSource("hdmi1".into())).unwrap();
        assert_eq!(tv.current_app(), None);
        assert_eq!(tv.playback(), PlaybackState::Idle);
    }

    #[test]
    fn seek_does_not_go_negative() {
        let mut tv = on_tv();
        tv.apply(MediaCommand::LaunchApp("netflix".into())).unwrap();
        tv.apply(MediaCommand::Play).unwrap();
        tv.apply(MediaCommand::SeekRelative(20)).unwrap();
        assert_eq!(tv.position_secs(), 20);
        tv.apply(MediaCommand::SeekRelative(-100)).unwrap();
        assert_eq!(tv.position_secs(), 0, "seek clamps at the start");
    }

    #[test]
    fn rejected_command_leaves_state_unchanged() {
        let mut tv = on_tv();
        let before = tv.clone();
        let _ = tv.apply(MediaCommand::SelectSource("scart".into()));
        assert_eq!(tv, before, "a rejected command is a no-op");
    }

    #[test]
    fn can_apply_is_a_pure_query() {
        let tv = Display::new(SourceCatalog::typical_smart_tv(), Volume::new(20));
        assert!(!tv.can_apply(&MediaCommand::Play));
        assert!(tv.can_apply(&MediaCommand::PowerOn));
    }
}
