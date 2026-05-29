//! `cave-home-notify` — the notification routing brain for cave-home (ADR-021).
//!
//! This crate is the **decision engine** that turns "something happened" into
//! "who gets told, how loudly, and over which channel" — all as pure, std-only
//! logic with no network, transport or clock dependency. It is the part of the
//! notification stack that every automation, Portal tile and (Phase-1b)
//! transport consumes.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`priority`] — the five-level [`Priority`] scale with ordering and a
//!   minimum-priority filter.
//! - [`topic`] — the validated [`Topic`] pub/sub channel name.
//! - [`notification`] — the [`Notification`] value model (title, body,
//!   priority, tags, click-action / attachment metadata, caller-supplied
//!   creation tick) and its content [`DedupKey`](notification::DedupKey).
//! - [`route`] — the [`Subscriber`] interest model, the delivery [`Channel`]
//!   abstraction, and [`route`](route::route) which produces a [`DeliveryPlan`]
//!   with per-subscriber priority filtering and per-channel capability gating.
//! - [`throttle`] — [`DedupCache`] (collapse identical notifications within a
//!   window) and [`RateLimiter`] (per-topic token bucket), both pure over a
//!   caller-supplied integer clock.
//! - [`label`] — EN / DE / TR grandma-friendly priority labels and
//!   notification rendering (Charter §6.3, ADR-007).
//!
//! The **transports and server** — the self-hosted ntfy-class HTTP/WebSocket
//! push server, the APNs/FCM-free self-hosted mobile push path (Charter §9),
//! the SMTP / SMS / webhook senders, and the cave-home-core event-bus
//! integration — are network-bound and deferred to Phase 1b. Every one is
//! enumerated in `parity.manifest.toml` `[[unmapped]]` with an ADR-021
//! disposition. They consume the [`DeliveryPlan`] this engine produces.
//!
//! Port method: the priority scale, topic/pub-sub model, dedup window and
//! token-bucket rate limiter follow well-known *self-hosted notification*
//! semantics (ntfy-class) but are implemented **first-party** — no upstream
//! source was read or ported.
//!
//! # Example
//!
//! ```
//! use cave_home_notify::{
//!     route, Channel, Lang, Notification, Priority, Subscriber, Topic,
//! };
//!
//! // A water-leak alert published to the "kitchen-leak" channel.
//! let leak = Topic::new("kitchen-leak").unwrap();
//! let alert = Notification::new(leak.clone(), "Water leak", "Under the sink", 0)
//!     .with_priority(Priority::High);
//!
//! // Mum wants anything on this channel by push; a quiet logger only takes
//! // urgent things over a webhook.
//! let subs = [
//!     Subscriber::new("mum", leak.clone(), Priority::Low, [Channel::Push]),
//!     Subscriber::new("logger", leak, Priority::Max, [Channel::Webhook]),
//! ];
//!
//! let plan = route(&alert, &subs);
//! // Mum is told; the logger's Max floor filters this High alert out.
//! assert_eq!(plan.recipients(), vec!["mum"]);
//!
//! // The household reads plain words, never a priority number.
//! assert_eq!(alert.render(Lang::En), "Important: Water leak — Under the sink");
//! ```

pub mod label;
pub mod notification;
pub mod priority;
pub mod route;
pub mod throttle;
pub mod topic;

pub use label::Lang;
pub use notification::{Attachment, ClickAction, DedupKey, Notification, Tick};
pub use priority::Priority;
pub use route::{route, Channel, Delivery, DeliveryPlan, Subscriber};
pub use throttle::{DedupCache, RateLimiter};
pub use topic::{Topic, TopicError};
