# Coverage matrix — cave-home-containerd-rs

**Declared:** fill=0.22 · adr_justified=all-entries · honest=0.22 · port method per manifest.
**Verified:** 34/34 mapped symbols found in source · 58 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| RuntimeService gRPC interface | src/cri/runtime_service.rs::RuntimeServer | yes |
| ImageService gRPC interface | src/cri/image_service.rs::ImageServer | yes |
| Sandbox store CRUD | src/cri/sandbox_store.rs::SandboxStore::{add, get, list, update_status, delete} | yes |
| Container store CRUD | src/cri/container_store.rs::ContainerStore::{add, get, list, list_for_sandbox, update_status, delete} | yes |
| RunPodSandbox RPC | src/cri/runtime_service.rs::RuntimeServer::run_pod_sandbox | yes |
| StopPodSandbox RPC | src/cri/runtime_service.rs::RuntimeServer::stop_pod_sandbox | yes |
| RemovePodSandbox RPC | src/cri/runtime_service.rs::RuntimeServer::remove_pod_sandbox | yes |
| PodSandboxStatus RPC | src/cri/runtime_service.rs::RuntimeServer::pod_sandbox_status | yes |
| ListPodSandbox RPC | src/cri/runtime_service.rs::RuntimeServer::list_pod_sandbox | yes |
| CreateContainer RPC | src/cri/runtime_service.rs::RuntimeServer::create_container | yes |
| StartContainer RPC | src/cri/runtime_service.rs::RuntimeServer::start_container | yes |
| StopContainer RPC | src/cri/runtime_service.rs::RuntimeServer::stop_container | yes |
| RemoveContainer RPC | src/cri/runtime_service.rs::RuntimeServer::remove_container | yes |
| ContainerStatus RPC | src/cri/runtime_service.rs::RuntimeServer::container_status | yes |
| ListContainers RPC | src/cri/runtime_service.rs::RuntimeServer::list_containers | yes |
| UpdateContainerResources RPC | src/cri/runtime_service.rs::RuntimeServer::update_container_resources | yes |
| Version RPC | src/cri/runtime_service.rs::RuntimeServer::version | yes |
| Status RPC | src/cri/runtime_service.rs::RuntimeServer::status | yes |
| RuntimeConfig RPC | src/cri/runtime_service.rs::RuntimeServer::runtime_config | yes |
| UpdateRuntimeConfig RPC | src/cri/runtime_service.rs::RuntimeServer::update_runtime_config | yes |
| PullImage RPC | src/cri/image_service.rs::ImageServer::pull_image | yes |
| ImageStatus RPC | src/cri/image_service.rs::ImageServer::image_status | yes |
| ListImages RPC | src/cri/image_service.rs::ImageServer::list_images | yes |
| RemoveImage RPC | src/cri/image_service.rs::ImageServer::remove_image | yes |
| ImageFsInfo RPC | src/cri/image_service.rs::ImageServer::image_fs_info | yes |
| Content store write/verify | src/content/store.rs::Store::{open, write, info, walk, exists, read, delete} | yes |
| Overlay snapshotter | src/snapshots/overlay.rs::Snapshotter::{prepare, view, commit, remove, stat, walk, mounts_for} | yes |
| Docker auth parsing | src/image/auth.rs::{parse_challenge, first_bearer} | yes |
| Docker resolver | src/image/resolver.rs::Resolver::{resolve_with_scheme, bearer_token, fetch_blob_with_scheme} | yes |
| gRPC server router | src/server.rs::router | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| runc/shim invocation | phase-1b-high | Container execution deferred; Phase 1 stops at metadata + state machine |
| OCI runtime-spec generator | phase-1b-high | Required once runc shim lands in Phase 1b |
| Linux namespace creation (CLONE_NEWNET) | phase-1b-high | Real namespace setup deferred; Phase 1b work |
| Exec/Attach/PortForward HTTP streaming | phase-1b-high | Streaming RPCs return gRPC unimplemented status; Phase 1b |
| Container cgroup v2 stats collection | phase-1b-medium | Stats collection deferred; Phase 1b metrics work |
| PodSandbox cgroup v2 stats collection | phase-1b-medium | Stats collection deferred; Phase 1b metrics work |
| Persistent metadata store (bbolt/sled) | phase-1b-medium | Phase 1 uses in-memory metastore; persistent backend Phase 1b |
| mount(2) syscall invocation | phase-1b-high | Phase 1 returns mount strings; actual mount syscall Phase 1b |
| btrfs snapshotter | phase-1b-low | Phase 1 ships overlayfs only; alternate snapshotters deferred |
| native snapshotter (cp -a) | phase-1b-low | Phase 1 ships overlayfs only |
| devicemapper snapshotter | phase-1b-low | Phase 1 ships overlayfs only |
| zfs snapshotter | phase-1b-low | Phase 1 ships overlayfs only |
| CNI plugin invocation | phase-1b-high | Needs cave-home-cni-flannel integration (M2); Phase 1b |
| CheckpointContainer / CRIU bridging | phase-1b-low | CRIU checkpoint feature deferred; Phase 1b lower priority |
| fsverity integrity probing | phase-1b-medium | Phase 1 content store skips fsverity; Phase 1b once kernel detection lands |
| Resumable/chunked blob ingest | phase-1b-medium | Phase 1 callers provide full byte slices; resumable ingest Phase 1b |
| Manifest-list multi-arch selection | phase-1b-medium | Phase 1 handles single-arch only; arch-aware selection Phase 1b |
| TruncIndex prefix-match lookup | phase-1b-low | Phase 1 requires full container ID; prefix lookup Phase 1b |
| SELinux label lifecycle store | phase-1b-medium | Phase 1 stores ProcessLabel as plain string; lifecycle Phase 1b |
| Transfer service (alternate image-pull) | phase-1b-low | Phase 1 ships CRI PullImage path only |
| cgroup v1 helpers | permanently-unmapped | Legacy linux; out of scope per Charter §3 (no backcompat) |
| Windows shim entry | permanently-unmapped | Linux-only per Charter §6 |
| FreeBSD runtime runner | permanently-unmapped | Linux-only per Charter §6 |
| Windows/macOS PodSandbox stubs | permanently-unmapped | Linux-only per Charter §6 |

## Drift notes
None — every claimed symbol exists in source. All 34 mapped entries verified in codebase. Symbol-count parity (34/60 = 0.567) honestly weighted down to fill_ratio = 0.22 due to unmapped items dominating execution surface area (streaming/exec/stats/mount operations).
