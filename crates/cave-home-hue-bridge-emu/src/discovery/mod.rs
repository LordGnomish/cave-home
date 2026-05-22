// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! Discovery surfaces — UPnP SSDP `<description.xml>` and mDNS
//! `_hue._tcp.local.` advertisement payloads.
//!
//! Reference:
//! - developers.meethue.com/develop/application-design-guidance/hue-bridge-discovery
//!   — every section of this file maps to a discovery method described
//!   there: NUPNP, mDNS, SSDP.
//! - Hue Bridge SSDP `<root>` schema reference: published in the dev
//!   portal "discovery" docs.

pub mod mdns;
pub mod ssdp;
