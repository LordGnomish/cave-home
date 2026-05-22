// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v1/__init__.py
//! Philips Hue v1 REST API client. Ports `aiohue.v1` line-by-line.
//!
//! v1 is the legacy "ColdRoom" REST API (JSON over HTTP, application-key in
//! the URL path). Most modern Hue Bridges (v2 / square) also expose v1; we
//! keep this surface to support round-bridge / firmware-frozen units.
//!
//! Modules mirror upstream files: [`api`] (`api.py`), [`lights`]
//! (`lights.py`), [`groups`] (`groups.py`), [`scenes`] (`scenes.py`),
//! [`sensors`] (`sensors.py`), [`config`] (`config.py`).

pub mod api;
pub mod bridge;
pub mod config;
pub mod groups;
pub mod lights;
pub mod scenes;
pub mod sensors;
