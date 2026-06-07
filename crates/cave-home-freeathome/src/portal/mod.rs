// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Portal page module for free@home devices.
//!
//! Builds the grandma-friendly viewmodel the cave-home portal renders: a tile
//! per device, a detail view with the controls a household actually uses, and a
//! sensor filter — all in jargon-free EN/DE/TR vocabulary (Charter §6.3).

pub mod viewmodel;

pub use viewmodel::{
    Control, DeviceDetailView, DeviceTile, controls, detail, kind_label, sensors, tile,
};
