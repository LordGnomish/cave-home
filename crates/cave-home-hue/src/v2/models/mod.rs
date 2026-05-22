// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/*
//! v2 CLIP resource models. One module per upstream `aiohue.v2.models.*` file.
//!
//! Phase 1 MVP includes the modules end-users touch (lights, scenes, motion,
//! buttons, devices, rooms/zones, grouped lights, bridges, batteries) plus
//! the generic [`resource`] enum + [`ResourceIdentifier`]. The longer tail
//! (camera_motion, security_area_motion, ...) is tracked as `[[unmapped]]`
//! in the parity manifest.

pub mod button;
pub mod device;
pub mod feature;
pub mod grouped_light;
pub mod light;
pub mod motion;
pub mod resource;
pub mod room;
pub mod scene;
pub mod zone;
