# cave-home — daily progress · 2026-06-07

_Generated 2026-06-07T21:10:26.276104+02:00 by cave-home-tracker._

## Aggregate completion (honest)

| Rollup | Real % | Δ vs prev |
|---|---:|---:|
| **Overall** | 1.3% | +1.3 |
| K3s | 1.7% | — |
| Smart-Home | 0.9% | — |

## k3s

| Subsystem | Upstream LOC | Port LOC | Ratio | Tests (P/F/I) | Stubs | Real % | Δ Real % |
|---|---:|---:|---:|---:|---:|---:|---:|
| kine | 9409 | 1346 | 14% | 70/0/0 | 0 | 14.3% | new |
| apiserver | 213847 | 2268 | 1% | 79/0/0 | 0 | 1.1% | new |
| scheduler | 106881 | 3242 | 3% | 83/0/0 | 0 | 3.0% | new |
| controller-manager | 140016 | 1532 | 1% | 69/0/0 | 2 | 1.0% | new |
| kubelet | 209198 | 4163 | 2% | 77/0/0 | 0 | 2.0% | new |
| kube-proxy | 45496 | 2941 | 6% | 0/0/0 | 2 | 0.0% | new |
| containerd | 33603 | 1271 | 4% | 64/0/0 | 1 | 3.5% | new |
| cni-flannel | 10902 | 1705 | 16% | 88/0/0 | 0 | 15.6% | new |
| coredns | 71316 | 0 | 0% | 0/0/0 | 0 | 0.0% | new |
| traefik | 175007 | 1335 | 1% | 74/0/0 | 2 | 0.6% | new |
| servicelb | 94 | 1535 | 100% | 52/0/0 | 5 | 67.4% | new |
| helm-controller | 3101 | 1013 | 33% | 54/0/0 | 0 | 32.7% | new |
| metrics-server | 8509 | 0 | 0% | 0/0/0 | 0 | 0.0% | new |
| local-path-provisioner | 2082 | 0 | 0% | 0/0/0 | 0 | 0.0% | new |
| **k3s aggregate** | | | | | | **1.7%** | |

## smart-home

| Subsystem | Upstream LOC | Port LOC | Ratio | Tests (P/F/I) | Stubs | Real % | Δ Real % |
|---|---:|---:|---:|---:|---:|---:|---:|
| mqtt | 10952 | 392 | 4% | 5/0/0 | 0 | 3.6% | new |
| hue | 5796 | 3035 | 52% | 67/0/0 | 0 | 52.4% | new |
| free-home | 6486 | 1625 | 25% | 59/0/0 | 0 | 25.1% | new |
| voice | 8563 | 2210 | 26% | 78/0/0 | 12 | 11.8% | new |
| automation | 35496 | 3350 | 9% | 51/0/0 | 2 | 8.9% | new |
| integrations | 1082330 | 1203 | 0% | 47/0/0 | 0 | 0.1% | new |
| **smart-home aggregate** | | | | | | **0.9%** | |

## Overall trend (last 30 snapshots)

```
▁
2026-06-07 1.3%  →  2026-06-07 1.3%
```

> Real % = coverage × test-pass-rate × stub-integrity. No paperwork.
