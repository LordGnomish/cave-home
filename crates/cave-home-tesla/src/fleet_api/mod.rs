// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Tesla **Fleet API** cloud control plane.
//!
//! - [`auth`] — `OAuth2` Authorization-Code + PKCE (S256) flow.
//! - [`client`] — the rate-limited, transport-injected request model.
//! - [`endpoints`] — the `/api/1/energy_sites/*` request builders.
//! - [`types`] — the wire DTOs returned by those endpoints.

pub mod auth;
pub mod client;
pub mod endpoints;
pub mod types;

/// A Tesla Fleet API region. The API is sharded by region; a token issued in
/// one region only works against that region's base host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Region {
    /// North America + Asia-Pacific (excluding China).
    #[default]
    NorthAmericaAsiaPacific,
    /// Europe, Middle East and Africa.
    Europe,
    /// Mainland China.
    China,
}

impl Region {
    /// The regional Fleet API base host (no trailing slash).
    #[must_use]
    pub const fn base_url(self) -> &'static str {
        match self {
            Self::NorthAmericaAsiaPacific => "https://fleet-api.prd.na.vn.cloud.tesla.com",
            Self::Europe => "https://fleet-api.prd.eu.vn.cloud.tesla.com",
            Self::China => "https://fleet-api.prd.cn.vn.cloud.tesla.cn",
        }
    }

    /// The stable config/flag key (`na`, `eu`, `cn`).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::NorthAmericaAsiaPacific => "na",
            Self::Europe => "eu",
            Self::China => "cn",
        }
    }

    /// Parse a [`Region`] from its config key.
    #[must_use]
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "na" => Some(Self::NorthAmericaAsiaPacific),
            "eu" => Some(Self::Europe),
            "cn" => Some(Self::China),
            _ => None,
        }
    }
}
