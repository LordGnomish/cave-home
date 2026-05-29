//! `cave-home-dns-adguard` — the DNS ad/tracker-blocking decision core (ADR-022).
//!
//! This crate is the **brain** that turns a household's blocklists into a verdict
//! for every DNS lookup: it parses the common public blocklist formats, compiles
//! them into a rule set, and decides whether a domain is blocked, allowed
//! (allowlist exception), or simply not filtered — plus DNS rewrites, query
//! statistics, and grandma-friendly EN / DE / TR phrasing.
//!
//! # Clean-room (Charter §6.1 / ADR-022)
//!
//! The upstream ad-blocker is GPL. This crate is implemented **only** from the
//! *public* filter-syntax documentation — Adblock-style domain rules,
//! `/etc/hosts` blackhole lines, plain domain lists, dnsmasq-style comments. The
//! upstream source was **not** read or ported.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`rule`] — the [`Rule`] model (block/allow, exact vs subdomain) + domain
//!   normalisation.
//! - [`parse`] — parsing for `/etc/hosts`, Adblock-style and plain-list formats,
//!   with comments/blanks/malformed lines skipped (never panics).
//! - [`engine`] — the [`RuleSet`] and [`RuleSet::decide`] matching engine, with
//!   allowlist precedence and household client overrides.
//! - [`rewrite`] — domain → fixed-answer DNS rewrites.
//! - [`stats`] — query/blocked aggregation over a passed-in query log.
//! - [`label`] — localised, jargon-free UX (Charter §6.3, ADR-007).
//!
//! The **DNS server transport** (UDP/TCP/DoH/DoT), the upstream recursive
//! resolver, blocklist auto-update fetch, the live query-interception loop and
//! cave-home-core integration are all network-bound and deferred to Phase 1b —
//! every one is enumerated in `parity.manifest.toml` `[[unmapped]]` with an
//! ADR-022 disposition. Per Charter §9 there is a **permanent** entry: no
//! cloud-relayed DNS.
//!
//! # Example
//!
//! ```
//! use cave_home_dns_adguard::{RuleSet, FilterDecision, Lang, blocked_today};
//!
//! let mut rules = RuleSet::new();
//! rules.add_blocklist("\
//! # my blocklist
//! ||ads.example.com^
//! @@||good.example.com^
//! ");
//!
//! // A tracker subdomain is blocked…
//! assert!(rules.decide("img.ads.example.com").is_blocked());
//! // …but the allowlist exception wins for the trusted name.
//! assert!(matches!(rules.decide("good.example.com"), FilterDecision::Allowed(_)));
//! // …and an untouched domain resolves normally.
//! assert_eq!(rules.decide("wikipedia.org"), FilterDecision::NotFiltered);
//!
//! // The household sees plain language, never the rule internals.
//! assert_eq!(blocked_today(1_240, Lang::En), "Blocked 1,240 ads today");
//! ```

pub mod engine;
pub mod label;
pub mod parse;
pub mod rewrite;
pub mod rule;
pub mod stats;

pub use engine::{FilterDecision, RuleSet};
pub use label::{
    Lang, blocked_today, protection_off, protection_on, site_allowed, site_blocked,
    site_not_filtered,
};
pub use parse::{parse_blocklist, parse_line};
pub use rewrite::{Answer, Rewrite, RewriteTable};
pub use rule::{Action, DomainPattern, Rule, normalize_domain};
pub use stats::{QueryEvent, Stats};
