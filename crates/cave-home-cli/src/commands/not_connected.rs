// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The honest answer for a verb whose backend is not wired into the CLI yet
//! (RED skeleton — the verbs are routed here in the next commit).

/// Exit code for "this verb's backend is not connected" — sysexits.h
/// `EX_UNAVAILABLE`, distinct from 1 (the operation failed) and 2 (usage).
pub const EXIT_NOT_CONNECTED: i32 = 69;
