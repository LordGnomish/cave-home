// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The validated assistant configuration: which model to talk to, which wake
//! words to listen for, and which microphone sits in which room.
//!
//! This is settings-only — it builds the [`RoomRegistry`] and [`DispatchConfig`]
//! the runtime pieces consume, so the household configures the assistant in one
//! place.

use cave_home_voice::Lang;

use crate::dispatch::DispatchConfig;
use crate::error::{JarvisError, Result};
use crate::room::RoomRegistry;

/// One microphone-to-room placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePlacement {
    /// The capture device id (microphone).
    pub device: String,
    /// The room it sits in.
    pub room: String,
}

/// The full assistant configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct JarvisConfig {
    /// The local model name (e.g. `llama3.1`).
    pub model: String,
    /// The local model server base URL (e.g. `http://127.0.0.1:11434`).
    pub base_url: String,
    /// The reply language for the NLU path.
    pub lang: Lang,
    /// The wake words to listen for.
    pub wake_keywords: Vec<String>,
    /// The DTW acceptance threshold for wake spotting.
    pub wake_threshold: f32,
    /// The cosine acceptance threshold for speaker identification.
    pub speaker_threshold: f32,
    /// Microphone placements.
    pub devices: Vec<DevicePlacement>,
    /// An optional system-prompt override for the LLM path.
    pub system_prompt: Option<String>,
}

impl Default for JarvisConfig {
    fn default() -> Self {
        Self {
            model: "llama3.1".into(),
            base_url: "http://127.0.0.1:11434".into(),
            lang: Lang::En,
            wake_keywords: vec!["jarvis".into()],
            wake_threshold: 0.55,
            speaker_threshold: 0.6,
            devices: Vec::new(),
            system_prompt: None,
        }
    }
}

impl JarvisConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    /// [`JarvisError::Config`] describing the first problem found.
    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() {
            return Err(JarvisError::Config("model name is empty".into()));
        }
        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            return Err(JarvisError::Config(format!(
                "base_url must be http(s): got '{}'",
                self.base_url
            )));
        }
        if self.wake_keywords.is_empty() {
            return Err(JarvisError::Config("at least one wake keyword is required".into()));
        }
        if !(0.0..=10.0).contains(&self.wake_threshold) {
            return Err(JarvisError::Config("wake_threshold out of range".into()));
        }
        if !(-1.0..=1.0).contains(&self.speaker_threshold) {
            return Err(JarvisError::Config("speaker_threshold out of [-1,1]".into()));
        }
        // No two devices may name the same id.
        for (i, d) in self.devices.iter().enumerate() {
            if self.devices[i + 1..].iter().any(|o| o.device == d.device) {
                return Err(JarvisError::Config(format!("duplicate device '{}'", d.device)));
            }
        }
        Ok(())
    }

    /// Build the room registry from the device placements.
    #[must_use]
    pub fn room_registry(&self) -> RoomRegistry {
        let mut reg = RoomRegistry::new();
        for d in &self.devices {
            reg.register(&d.device, &d.room);
        }
        reg
    }

    /// Build the dispatch configuration (carrying any system-prompt override).
    #[must_use]
    pub fn dispatch_config(&self) -> DispatchConfig {
        let mut cfg = DispatchConfig {
            model: self.model.clone(),
            lang: self.lang,
            ..DispatchConfig::default()
        };
        if let Some(p) = &self.system_prompt {
            cfg.system_prompt.clone_from(p);
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> JarvisConfig {
        JarvisConfig {
            devices: vec![
                DevicePlacement { device: "mic-kitchen".into(), room: "kitchen".into() },
                DevicePlacement { device: "mic-office".into(), room: "office".into() },
            ],
            ..JarvisConfig::default()
        }
    }

    #[test]
    fn default_is_valid() {
        assert!(JarvisConfig::default().validate().is_ok());
    }

    #[test]
    fn rejects_empty_model() {
        let mut c = cfg();
        c.model = "  ".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_non_http_base_url() {
        let mut c = cfg();
        c.base_url = "ftp://nope".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_no_wake_keyword() {
        let mut c = cfg();
        c.wake_keywords.clear();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_device() {
        let mut c = cfg();
        c.devices.push(DevicePlacement { device: "mic-kitchen".into(), room: "den".into() });
        assert!(c.validate().is_err());
    }

    #[test]
    fn builds_room_registry() {
        let reg = cfg().room_registry();
        assert_eq!(reg.room_of("mic-office").unwrap(), "office");
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn dispatch_config_carries_model_and_prompt_override() {
        let mut c = cfg();
        c.model = "qwen2.5".into();
        c.system_prompt = Some("Be terse.".into());
        let dc = c.dispatch_config();
        assert_eq!(dc.model, "qwen2.5");
        assert_eq!(dc.system_prompt, "Be terse.");
    }
}
