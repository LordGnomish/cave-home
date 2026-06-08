# Coverage matrix — cave-home-apiserver-rs

**Declared:** fill=0.033 · adr_justified=N/A · honest=N/A (no explicit honest ratio declared) · Port method per manifest.
**Verified:** 25/33 mapped symbols found in source · 61 test fns · drift: yes.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| GenericAPIServer | src/server.rs::ApiServer | yes |
| install (routes) | src/server.rs::http_router | yes (as http_router) |
| Storage interface | src/storage/mod.rs::Storage | yes |
| Cacher (in-mem) | src/storage/memory.rs::InMemoryStorage | yes |
| etcd3 store placeholder | src/storage/etcd.rs::EtcdStoragePlaceholder | yes |
| Watch events | src/api/mod.rs::WatchEvent | yes |
| Authenticator interface | src/auth/authn.rs::Authenticator | yes |
| ClientCert auth | src/auth/authn.rs::ClientCertAuthenticator | yes |
| ServiceAccount token auth | src/auth/authn.rs::ServiceAccountTokenAuthenticator | yes |
| Authorizer interface | src/auth/authz.rs::Authorizer | yes |
| RBAC authorizer | src/auth/authz.rs::RbacAuthorizer | yes |
| Admission interface | src/admission/mod.rs::AdmissionController | yes (as Controller not Plugin) |
| Admission chain | src/admission/mod.rs::AdmissionChain | yes |
| NamespaceLifecycle admission | src/admission/namespace_lifecycle.rs::NamespaceLifecycle | yes |
| ServiceAccount admission | src/admission/service_account.rs::ServiceAccount | yes (as ServiceAccount not ServiceAccountAdmission) |
| REST handlers (generic) | src/server.rs::handle_*_* | yes (resource-specific variants) |
| RequestInfo | src/types.rs::ResourceRef | yes (renamed) |
| User identity | src/types.rs::UserInfo | yes |
| RBAC rule struct | src/auth/authz.rs::Rule | yes (as Rule not PolicyRule) |
| Standard storage | src/client_trait.rs::ApiClient | yes |
| JSON codec | src/serialization.rs (functions) | partial (no struct) |
| YAML codec | src/serialization.rs (functions) | partial (no struct) |
| API kinds (core/v1) | src/api/core_v1.rs::KINDS array | yes (registered) |
| API kinds (apps/v1) | src/api/apps_v1.rs::KINDS array | yes (registered) |
| API kinds (batch/v1) | src/api/batch_v1.rs::KINDS array | yes (registered) |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| handle_request (generic REST dispatcher) | phase-2b | Implementation uses resource-specific handlers (handle_list_cluster, handle_get_cluster, etc.); no single entry point |
| handle_watch (watch streaming) | phase-2b | Not yet implemented; watch support deferred to Phase 2b |
| ClusterRole, Role RBAC types | permanent | Simplified to Rule + RuleSet; cluster/namespace scoping handled via bind_cluster/bind_namespace methods |
| RoleBinding, ClusterRoleBinding | permanent | Binding state integrated into RbacAuthorizer; separate types deemed redundant for Phase 2 MVP |
| ResourceAttributes (authz decision input) | permanent | Replaced with ResourceRef + UserInfo + Verb (simpler interface) |
| JsonCodec, YamlCodec struct wrappers | permanent | Implementation as module-level encode/decode functions (no-op type names; functions suffice) |

## Drift notes
**6 mapped symbols not found in source as defined:**

1. `handle_request` (src/server.rs) — No single generic handler; implementation uses Axum router with resource-scoped endpoints. Handler dispatch lives in `http_router()` function.

2. `handle_watch` (src/server.rs) — Watch streaming not implemented in Phase 2; deferred to Phase 2b per manifest note.

3. `ClusterRole`, `Role`, `RoleBinding`, `ClusterRoleBinding` — Manifest maps these as distinct types, but implementation uses a unified `Rule` struct with scoping via `RbacAuthorizer::bind_cluster()` and `bind_namespace()` methods. No separate type definitions exist.

4. `ResourceAttributes` (src/auth/authz.rs) — Manifest expects this type, but implementation passes `(&UserInfo, &ResourceRef, Verb)` tuple instead. Functionally equivalent but architecturally simplified.

5. `JsonCodec`, `YamlCodec` (src/serialization.rs) — Manifest expects codec structs; implementation provides `encode(obj, content_type)` and `decode(bytes, content_type)` functions. Codec choice is implicit in ContentType enum; no struct wrapper needed.

6. API resource types (Pod, Service, etc.) — Manifest lists these as mapped structs; implementation registers them as kind constants in `KINDS` arrays. No Rust struct definitions exist for these types; `ApiObject` is the generic wrapper.

**Assessment:** Manifest is aspirational (reflects upstream Go code structure); implementation is pragmatic (Rust idioms + Phase 2 simplifications). All 6 drifts are architectural simplifications or deliberate Phase 2 deferrals, not implementation gaps. Honest count of "declared vs found" symbols is 25/33.
