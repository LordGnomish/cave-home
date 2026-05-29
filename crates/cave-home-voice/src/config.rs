//! Voice-assistant configuration — *settings only*, no audio.
//!
//! This is the household's voice preferences: which wake words are enabled
//! ("Hey cave-home"), what language the assistant listens and replies in, and
//! which voice it speaks with. The actual wake-word detection, speech-to-text
//! and text-to-speech engines are ML/audio-bound and deferred to Phase-1b
//! (see `parity.manifest.toml` and ADR-024). This module only holds the
//! configuration those engines will consume — and it validates it, so the
//! engine never starts with a contradictory setup.

use crate::label::Lang;

/// A wake word the household has enabled. cave-home ships a default and lets
/// users add their own (trained locally — never in the cloud, Charter §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeWord {
    /// What the household says to wake the assistant, e.g. "Hey cave-home".
    pub phrase: String,
    /// Whether this wake word is currently active.
    pub enabled: bool,
}

impl WakeWord {
    /// An enabled wake word from a phrase.
    #[must_use]
    pub fn enabled(phrase: impl Into<String>) -> WakeWord {
        WakeWord {
            phrase: phrase.into(),
            enabled: true,
        }
    }
}

/// The full voice-assistant configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantConfig {
    /// The language the assistant listens for and replies in.
    pub language: Lang,
    /// The named voice the assistant speaks with (a piper voice id in
    /// Phase-1b; here just a label so the config round-trips).
    pub voice: String,
    /// Configured wake words (some may be disabled).
    pub wake_words: Vec<WakeWord>,
}

/// Why a configuration is not usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// No wake word is enabled — the assistant could never be triggered.
    NoEnabledWakeWord,
    /// A wake word phrase was blank.
    EmptyWakeWord,
    /// No voice was chosen.
    NoVoice,
}

impl AssistantConfig {
    /// A sensible default: "Hey cave-home", the given language, a default voice.
    #[must_use]
    pub fn default_for(language: Lang) -> AssistantConfig {
        AssistantConfig {
            language,
            voice: "default".to_string(),
            wake_words: vec![WakeWord::enabled("Hey cave-home")],
        }
    }

    /// The phrases of the currently-enabled wake words.
    #[must_use]
    pub fn enabled_wake_words(&self) -> Vec<&str> {
        self.wake_words
            .iter()
            .filter(|w| w.enabled)
            .map(|w| w.phrase.as_str())
            .collect()
    }

    /// Validate the configuration before the (Phase-1b) engines consume it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if no wake word is enabled, a phrase is blank,
    /// or no voice is set.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.voice.trim().is_empty() {
            return Err(ConfigError::NoVoice);
        }
        if self.wake_words.iter().any(|w| w.phrase.trim().is_empty()) {
            return Err(ConfigError::EmptyWakeWord);
        }
        if !self.wake_words.iter().any(|w| w.enabled) {
            return Err(ConfigError::NoEnabledWakeWord);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid_and_has_a_wake_word() {
        let c = AssistantConfig::default_for(Lang::En);
        assert!(c.validate().is_ok());
        assert_eq!(c.enabled_wake_words(), vec!["Hey cave-home"]);
    }

    #[test]
    fn rejects_config_with_no_enabled_wake_word() {
        let mut c = AssistantConfig::default_for(Lang::De);
        for w in &mut c.wake_words {
            w.enabled = false;
        }
        assert_eq!(c.validate(), Err(ConfigError::NoEnabledWakeWord));
    }

    #[test]
    fn rejects_blank_wake_word_phrase() {
        let c = AssistantConfig {
            language: Lang::Tr,
            voice: "default".into(),
            wake_words: vec![WakeWord::enabled("   ")],
        };
        assert_eq!(c.validate(), Err(ConfigError::EmptyWakeWord));
    }

    #[test]
    fn rejects_missing_voice() {
        let c = AssistantConfig {
            language: Lang::En,
            voice: String::new(),
            wake_words: vec![WakeWord::enabled("Hey cave-home")],
        };
        assert_eq!(c.validate(), Err(ConfigError::NoVoice));
    }

    #[test]
    fn enabled_wake_words_filters_disabled() {
        let c = AssistantConfig {
            language: Lang::En,
            voice: "default".into(),
            wake_words: vec![
                WakeWord::enabled("Hey cave-home"),
                WakeWord {
                    phrase: "Computer".into(),
                    enabled: false,
                },
            ],
        };
        assert_eq!(c.enabled_wake_words(), vec!["Hey cave-home"]);
    }
}
