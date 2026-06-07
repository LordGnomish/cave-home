// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/*
//! v2 controllers — one per resource type. Each holds a typed map of
//! resources + the PUT helpers that mutate the bridge.

pub mod base;
pub mod lights;
pub mod scenes;
