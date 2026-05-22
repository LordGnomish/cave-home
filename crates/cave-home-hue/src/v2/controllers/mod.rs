// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/controllers/*
//! v2 controllers — one per resource type. Each holds a typed map of
//! resources + the PUT helpers that mutate the bridge.

pub mod base;
pub mod lights;
pub mod scenes;
