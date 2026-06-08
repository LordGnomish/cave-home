# Coverage matrix — cave-home-notify

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 9/9 mapped symbols found in source · 40 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Five-level priority scale (1..=5) with ordering + minimum-priority filter | src/priority.rs::Priority | yes |
| Validated pub/sub channel name (non-empty, length-bounded, restricted charset) | src/topic.rs::Topic | yes |
| Notification value model (title/body/priority/tags/click-action/attachment/created-at) | src/notification.rs::Notification | yes |
| Content de-dup fingerprint (topic+title+body) | src/notification.rs::Notification::dedup_key | yes |
| Subscription model (topic interest + minimum priority + receivable channels) | src/route.rs::Subscriber | yes |
| Delivery-channel abstraction (push/email/sms/webhook) + capability gating | src/route.rs::Channel,route | yes |
| Routing decision producing a DeliveryPlan (priority filter + per-channel fan-out) | src/route.rs::DeliveryPlan | yes |
| De-duplication within a caller-supplied window (pure over integer clock) | src/throttle.rs::DedupCache | yes |
| Per-topic token-bucket rate limiter (burst + steady refill, pure over integer clock) | src/throttle.rs::RateLimiter | yes |
| Six-language-line grandma-friendly priority labels + notification rendering (ADR-007) | src/label.rs::Lang,Priority::label,Notification::render | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Self-hosted ntfy-class HTTP/WebSocket push server | phase-1b | ADR-021: the cave-home back-end relay that satisfies Charter §9 'no third-party push relay'. Network-bound (HTTP/WebSocket listener); it publishes onto the topics this engine routes and emits the DeliveryPlan's Push deliveries. No new decision logic, just the server I/O. |
| Self-hosted mobile push transport (APNs/FCM-free control plane) | phase-1b | ADR-021 + Charter §9: the cave-home control plane must not depend on Google/Apple; the OS-level final-hop wake-token is out-of-band. This transport carries a Channel::Push delivery to the companion app. Network/device-bound I/O adapter only. |
| SMTP / SMS / webhook transports | phase-1b | ADR-021: the actual senders behind Channel::Email / Channel::Sms / Channel::Webhook. Each consumes a Delivery from the plan and performs network I/O; no routing logic lives there. Modelled now as routing decisions only. |
| cave-home-core event-bus integration | phase-1b | ADR-021: turning core events (door opened, leak detected, alarm armed) into Notifications and feeding subscriber registries from core entities lands once cave-home-core's event API stabilises. The engine is already core-agnostic. |
| Persisted subscriber registry + message history store | phase-2 | ADR-021: durable storage of subscriptions and a queryable notification log is a storage-bound Phase 2 concern over the same pure model; the engine takes subscribers by slice and is storage-agnostic. |
| Third-party push-relay compatibility shim | permanent | Charter §9 no third-party push relay + Charter §8 no-backcompat: cave-home self-hosts its push surface; it will never ship a shim that routes the control plane through a third-party relay. |

## Drift notes
None — every claimed symbol exists in source.
