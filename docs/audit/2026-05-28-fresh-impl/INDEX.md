# Coverage matrix — fresh-impl audit (2026-05-28)

Generated 62 per-crate matrices from a read-only audit at `94f8dec`. Each agent parsed the crate's `parity.manifest.toml`, then verified every `[[mapped]]` `ours=` symbol actually exists in source (anti-drift check), counted test fns, and listed the ADR-dispositioned gaps.

**Honest-ratio program reminder:** `honest = fill / (fill + (1-fill)*(1-adr_justified))`. A gap only stops hurting the score if it carries a `priority` + `note` disposition; paperwork cannot raise the score above real `fill`.

## Per-crate index

| Crate | fill | honest | mapped verified | tests | drift |
|---|---:|---:|:---:|---:|:---:|
| [cave-home-air-quality](cave-home-air-quality.md) | 0.30 | 1.00 | 8/8 | 29 | — |
| [cave-home-alarm](cave-home-alarm.md) | 0.27 | 1.00 | 11/11 | 59 | — |
| [cave-home-apiserver-rs](cave-home-apiserver-rs.md) | 0.03 | 0.03 | 25/33 | 61 | ⚠️ |
| [cave-home-audio-mass](cave-home-audio-mass.md) | 0.30 | 1.00 | 26/26 | 62 | — |
| [cave-home-audio-mopidy](cave-home-audio-mopidy.md) | 0.30 | 1.00 | 7/7 | 58 | — |
| [cave-home-audio-snapcast](cave-home-audio-snapcast.md) | 0.30 | 1.00 | 32/32 | 56 | — |
| [cave-home-automation](cave-home-automation.md) | 0.42 | 1.00 | 67/67 | 50 | — |
| [cave-home-binary](cave-home-binary.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-calendar](cave-home-calendar.md) | 0.30 | 1.00 | 10/10 | 63 | — |
| [cave-home-camera](cave-home-camera.md) | 0.30 | 1.00 | 14/14 | 63 | — |
| [cave-home-cli](cave-home-cli.md) | 1.00 | 1.00 | 13/13 | 136 | — |
| [cave-home-cluster](cave-home-cluster.md) | 0.35 | 1.00 | 0/0 | 65 | — |
| [cave-home-cni-flannel](cave-home-cni-flannel.md) | 0.34 | 0.34 | 28/28 | 62 | — |
| [cave-home-containerd-rs](cave-home-containerd-rs.md) | 0.22 | 0.22 | 34/34 | 58 | — |
| [cave-home-controller-manager-rs](cave-home-controller-manager-rs.md) | 0.05 | 0.05 | 38/46 | 79 | ⚠️ |
| [cave-home-core](cave-home-core.md) | 0.46 | 0.46 | 8/8 | 9 | — |
| [cave-home-cover](cave-home-cover.md) | 0.30 | 1.00 | 11/12 | 35 | (fp) |
| [cave-home-display](cave-home-display.md) | 0.30 | 1.00 | 6/6 | 44 | — |
| [cave-home-dns-adguard](cave-home-dns-adguard.md) | 0.30 | 1.00 | 9/9 | 44 | — |
| [cave-home-dns-unbound](cave-home-dns-unbound.md) | 0.30 | 1.00 | 8/8 | 46 | — |
| [cave-home-doorbell](cave-home-doorbell.md) | 0.30 | 1.00 | 10/10 | 56 | — |
| [cave-home-free-home](cave-home-free-home.md) | 0.35 | 1.00 | 30/30 | 59 | (fp) |
| [cave-home-garden](cave-home-garden.md) | 0.30 | 1.00 | 9/9 | 36 | — |
| [cave-home-helm-controller-rs](cave-home-helm-controller-rs.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-history](cave-home-history.md) | 0.30 | 1.00 | 10/10 | 76 | — |
| [cave-home-household](cave-home-household.md) | 0.40 | 1.00 | 9/9 | 35 | — |
| [cave-home-hue](cave-home-hue.md) | 0.50 | 1.00 | 38/38 | 67 | — |
| [cave-home-hue-bridge-emu](cave-home-hue-bridge-emu.md) | 0.50 | 1.00 | 17/17 | 53 | — |
| [cave-home-hvac](cave-home-hvac.md) | 0.30 | 1.00 | 7/7 | 39 | — |
| [cave-home-integrations](cave-home-integrations.md) | 0.35 | 1.00 | 11/11 | 47 | — |
| [cave-home-kine-rs](cave-home-kine-rs.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-klipper-lb-rs](cave-home-klipper-lb-rs.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-knx](cave-home-knx.md) | 0.35 | 1.00 | 17/17 | 49 | — |
| [cave-home-kube-proxy-rs](cave-home-kube-proxy-rs.md) | 0.12 | 0.17 | 20/20 | 72 | — |
| [cave-home-kubelet-rs](cave-home-kubelet-rs.md) | 0.04 | 1.00 | 35/35 | 89 | — |
| [cave-home-lighting-wled](cave-home-lighting-wled.md) | 0.30 | 1.00 | 10/10 | 50 | — |
| [cave-home-lock](cave-home-lock.md) | 0.28 | 1.00 | 12/12 | 40 | — |
| [cave-home-matter](cave-home-matter.md) | 0.48 | 1.00 | 54/54 | 78 | — |
| [cave-home-mobile](cave-home-mobile.md) | 0.00 | 0.00 | 0/0 | 2 | — |
| [cave-home-mqtt](cave-home-mqtt.md) | 0.44 | 1.00 | 16/16 | 5 | — |
| [cave-home-node-discovery](cave-home-node-discovery.md) | 0.30 | 1.00 | 10/10 | 75 | — |
| [cave-home-notify](cave-home-notify.md) | 0.30 | 1.00 | 9/9 | 40 | — |
| [cave-home-orchestration](cave-home-orchestration.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-pool](cave-home-pool.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-portal](cave-home-portal.md) | 0.00 | 0.00 | 0/0 | 33 | (fp) |
| [cave-home-scheduler-rs](cave-home-scheduler-rs.md) | 0.10 | 0.11 | 37/37 | 83 | — |
| [cave-home-solar-evcc](cave-home-solar-evcc.md) | 0.30 | 1.00 | 10/10 | 59 | — |
| [cave-home-solar-forecast](cave-home-solar-forecast.md) | 0.45 | 1.00 | 22/22 | 61 | — |
| [cave-home-solar-hoymiles](cave-home-solar-hoymiles.md) | 0.30 | 1.00 | 11/11 | 44 | — |
| [cave-home-solar-sunspec](cave-home-solar-sunspec.md) | 0.28 | 1.00 | 9/9 | 50 | — |
| [cave-home-traefik-rs](cave-home-traefik-rs.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-unifi-access](cave-home-unifi-access.md) | 0.30 | 1.00 | 8/8 | 75 | — |
| [cave-home-unifi-network](cave-home-unifi-network.md) | 0.35 | 1.00 | 9/9 | 49 | — |
| [cave-home-unifi-protect](cave-home-unifi-protect.md) | 0.30 | 1.00 | 9/9 | 56 | — |
| [cave-home-unifi-talk](cave-home-unifi-talk.md) | 0.30 | 1.00 | 11/11 | 76 | — |
| [cave-home-vacuum](cave-home-vacuum.md) | 0.30 | 1.00 | 9/9 | 63 | — |
| [cave-home-voice](cave-home-voice.md) | 0.30 | 1.00 | 9/9 | 78 | — |
| [cave-home-water](cave-home-water.md) | 0.30 | 1.00 | 8/8 | 36 | — |
| [cave-home-wearable](cave-home-wearable.md) | 0.00 | 0.00 | 0/0 | 0 | — |
| [cave-home-wellness](cave-home-wellness.md) | 0.30 | 1.00 | 21/8 | 31 | — |
| [cave-home-zigbee](cave-home-zigbee.md) | 0.45 | 1.00 | 23/23 | 121 | — |
| [cave-home-zwave](cave-home-zwave.md) | 0.30 | 1.00 | 15/15 | 47 | — |

## Drift findings (manifest claims vs. source)

Real drift — `[[mapped]]` entries referencing symbols absent from source (should be reclassified as planned/unmapped):

- **cave-home-apiserver-rs** (25/33 verified): missing `handle_request`, `handle_watch`, `ClusterRole`, `Role`, `RoleBinding`, `ClusterRoleBinding`, `ResourceAttributes`, `JsonCodec`, `YamlCodec`
- **cave-home-controller-manager-rs** (38/46 verified): missing `src/controllers/deployment.rs::DeploymentController::rollout_rolling`, `src/controllers/replicaset.rs::ReplicaSetController::manage_replicas`, `src/controllers/daemonset.rs::node_should_run`, `src/controllers/statefulset.rs::StatefulSetController::ensure_ordered_pods`, `src/controllers/job.rs::JobController::manage_pods`, `src/controllers/cronjob.rs::next_schedule_time`, `src/controllers/garbage_collector.rs::GarbageCollector::process_graph_changes`

False positives / in-progress (verified by hand, NOT real committed drift):

- **cave-home-cover**: `direction` exists as a private fn (machine.rs:223); agent grep missed non-pub helper
- **cave-home-free-home**: uncommitted wave-9 working-tree code present; committed manifest still fill=0.0 (in-progress, not a committed defect)
- **cave-home-portal**: scaffold crate, 0/0 mapped

## Totals

- Crates audited: **62**
- Total test fns across all crates: **3009**
- Crates with real committed manifest drift: **2** (cave-home-apiserver-rs, cave-home-controller-manager-rs)

