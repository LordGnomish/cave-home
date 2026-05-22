// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/__init__.py
//! Philips Hue v2 (CLIP) API client. Ports `aiohue.v2` line-by-line.
//!
//! The v2 surface is a flat CLIP REST API + a persistent Server-Sent Events
//! stream. Resource types are richer than v1 (lights have separate `dimming`
//! / `color` / `color_temperature` services, scenes are first-class
//! objects, motion / button / temperature each get their own resource).
//!
//! - [`models`]      — typed resource structs (lights, scenes, devices,
//!                     motion, buttons, ...). 1:1 with `aiohue.v2.models.*`.
//! - [`controllers`] — request-scoped controllers wrapping `clip/v2/...`
//!                     endpoints. 1:1 with `aiohue.v2.controllers.*`.
//! - [`events`]      — typed wrapping of the EventStream payloads.
//! - [`bridge`]      — high-level `HueBridgeV2`.

pub mod bridge;
pub mod controllers;
pub mod events;
pub mod models;
