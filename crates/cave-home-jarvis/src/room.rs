// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Multi-room device context.
//!
//! Every microphone in the house lives in a room; when a wake word fires on one
//! device, the dispatcher needs to know *where* so "turn on the lights" means
//! the right room's lights.
//!
//! [`RoomRegistry`] maps devices to rooms, builds a [`DispatchContext`] for the
//! device that woke, and resolves deictic targets ("here", "this room") to the
//! concrete room name.

use std::collections::HashMap;

use crate::dispatch::DispatchContext;
use crate::error::{JarvisError, Result};

/// Words a household says to mean "the room I'm standing in".
const DEICTIC_TARGETS: [&str; 5] = ["here", "this room", "the room", "in here", "this area"];

/// A map from capture devices (microphones) to the rooms they sit in.
#[derive(Debug, Clone, Default)]
pub struct RoomRegistry {
    device_to_room: HashMap<String, String>,
}

impl RoomRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Place a device in a room (builder-style).
    #[must_use]
    pub fn with_device(mut self, device: impl Into<String>, room: impl Into<String>) -> Self {
        self.device_to_room.insert(device.into(), room.into());
        self
    }

    /// Place a device in a room.
    pub fn register(&mut self, device: impl Into<String>, room: impl Into<String>) {
        self.device_to_room.insert(device.into(), room.into());
    }

    /// Number of registered devices.
    #[must_use]
    pub fn len(&self) -> usize {
        self.device_to_room.len()
    }

    /// Whether no devices are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.device_to_room.is_empty()
    }

    /// The room a device sits in.
    ///
    /// # Errors
    /// [`JarvisError::UnknownRoom`] if the device was never registered.
    pub fn room_of(&self, device: &str) -> Result<&str> {
        self.device_to_room
            .get(device)
            .map(String::as_str)
            .ok_or_else(|| JarvisError::UnknownRoom(device.to_string()))
    }

    /// Every device in a room (sorted, for determinism).
    #[must_use]
    pub fn devices_in(&self, room: &str) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .device_to_room
            .iter()
            .filter(|(_, r)| r.as_str() == room)
            .map(|(d, _)| d.as_str())
            .collect();
        v.sort_unstable();
        v
    }

    /// Build the dispatch context for the device that woke, optionally tagging
    /// the recognised speaker.
    ///
    /// # Errors
    /// [`JarvisError::UnknownRoom`] if the device is unknown.
    pub fn context_for(&self, device: &str, speaker: Option<String>) -> Result<DispatchContext> {
        let room = self.room_of(device)?.to_string();
        Ok(DispatchContext {
            room: Some(room),
            speaker,
        })
    }

    /// Resolve a spoken target against the device's room: a deictic word
    /// ("here", "this room") becomes the concrete room name; anything else is
    /// returned unchanged.
    ///
    /// # Errors
    /// [`JarvisError::UnknownRoom`] if a deictic target is used from an unknown
    /// device (we cannot know which room "here" is).
    pub fn resolve_target(&self, device: &str, spoken_target: &str) -> Result<String> {
        let lowered = spoken_target.trim().to_lowercase();
        if DEICTIC_TARGETS.contains(&lowered.as_str()) {
            Ok(self.room_of(device)?.to_string())
        } else {
            Ok(spoken_target.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> RoomRegistry {
        RoomRegistry::new()
            .with_device("mic-kitchen", "kitchen")
            .with_device("mic-living", "living room")
            .with_device("mic-office", "office")
    }

    #[test]
    fn room_of_known_device() {
        assert_eq!(registry().room_of("mic-office").unwrap(), "office");
    }

    #[test]
    fn room_of_unknown_device_errors() {
        assert!(matches!(
            registry().room_of("mic-attic").unwrap_err(),
            JarvisError::UnknownRoom(_)
        ));
    }

    #[test]
    fn devices_in_room_are_listed() {
        let r = registry().with_device("mic-living-2", "living room");
        assert_eq!(r.devices_in("living room"), vec!["mic-living", "mic-living-2"]);
        assert_eq!(r.len(), 4);
    }

    #[test]
    fn context_carries_room_and_speaker() {
        let ctx = registry().context_for("mic-kitchen", Some("Sanja".into())).unwrap();
        assert_eq!(ctx.room.as_deref(), Some("kitchen"));
        assert_eq!(ctx.speaker.as_deref(), Some("Sanja"));
    }

    #[test]
    fn deictic_target_resolves_to_room() {
        let r = registry();
        assert_eq!(r.resolve_target("mic-office", "here").unwrap(), "office");
        assert_eq!(r.resolve_target("mic-office", "this room").unwrap(), "office");
    }

    #[test]
    fn explicit_target_is_unchanged() {
        let r = registry();
        assert_eq!(r.resolve_target("mic-office", "the garage").unwrap(), "the garage");
    }
}
