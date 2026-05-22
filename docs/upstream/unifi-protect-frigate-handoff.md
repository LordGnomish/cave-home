# UniFi Protect ↔ Frigate handoff

Source-of-truth contract for the seam between **`cave-home-unifi-protect`**
(port of HA `unifiprotect`) and **`cave-home-camera`** (port of
`blakeblackshear/frigate`). Mandated by ADR-009 §5.2 and open
question 2.

Last updated: 2026-05-17.

---

## Why the seam exists

A cave-home install often has two camera sources at once:

1. **UniFi Protect cameras** — G4 / G5 / G6 devices adopted by a UniFi
   Protect NVR. They speak the Ubiquiti REST + WebSocket protocol and
   ship their own smart-detect AI (person, vehicle, package, animal,
   licence-plate).
2. **Generic RTSP cameras** — third-party Reolink, Hikvision,
   Amcrest, Ezviz, no-name. They have an RTSP stream and nothing else.
   The AI inference happens in `cave-home-camera`'s Frigate pipeline.

A grandma in §2 persona 1 cannot tell the difference between the two —
both are "kameralar" to her, and both surface motion / smart-detection
events that drive automations. The seam ensures that:

- The Portal renders both kinds through **one** camera grid
  (`/admin/unifi/protect` and `/admin/camera` are bound to the same
  underlying tile component).
- The automation engine receives **one** event-stream protocol
  (`cave_home_automation::events::CameraEvent`) regardless of which
  detector fired.
- The user can override per-camera which subsystem owns AI inference
  on a per-camera basis. (E.g. a UniFi G4 owner who prefers Frigate's
  YOLO model for finer labels picks `FrigateMl` for that camera and
  the Protect-side smart-detect is silenced for it.)

---

## Layer ownership

```
                                      ┌──────────────────────────────┐
                                      │  cave-home-portal `/admin/*` │
                                      │  (one camera grid, one event │
                                      │   timeline, grandma labels)  │
                                      └─────────────┬────────────────┘
                                                    │
                       ┌────────────────────────────┴────────────────────────────┐
                       │                                                         │
              ┌────────▼─────────┐                                  ┌────────────▼───────────┐
              │ cave-home-camera │                                  │ cave-home-unifi-protect │
              │ (Frigate port)   │                                  │ (HA unifiprotect port)  │
              │                  │                                  │                         │
              │ - RTSP capture   │                                  │ - REST bootstrap        │
              │ - YOLO / Coral / │                                  │ - WS subscription        │
              │   TensorRT ML    │                                  │ - smartDetect events    │
              │ - motion / track │                                  │ - doorbell ring events  │
              │ - recording      │                                  │ - PTZ / privacy zone    │
              │ - event sink     │                                  │ - native camera control │
              └──────────┬───────┘                                  └────────────┬────────────┘
                         │                                                       │
                         └─────────────────────┬─────────────────────────────────┘
                                               │
                                  ┌────────────▼────────────┐
                                  │   FrigateSeam table     │
                                  │   (per-camera owner)    │
                                  └────────────┬────────────┘
                                               │
                                  ┌────────────▼────────────┐
                                  │ cave-home-automation    │
                                  │  EventBus (one stream)  │
                                  └─────────────────────────┘
```

---

## The `FrigateSeam` table

`cave_home_unifi_protect::frigate_seam::FrigateSeam` is a
per-camera ownership table mapping `CameraId` → `ProtectSubsystem`.
The three subsystem values:

| Variant         | Stream owner   | Inference owner | Typical user                                                       |
| --------------- | -------------- | --------------- | ------------------------------------------------------------------ |
| `Native`        | UniFi Protect  | UniFi Protect   | Pure-Ubiquiti household; G4/G5 cameras on a Protect NVR.           |
| `FrigateMl`     | UniFi Protect  | Frigate         | Has Protect cameras but prefers Frigate's larger label catalogue.  |
| `FrigateOnly`   | RTSP-direct    | Frigate         | Third-party RTSP camera; not adopted by any Protect NVR.           |

When `seam.owner_of(camera_id)` returns `None`, the camera is unknown
to the seam — the portal surfaces a yellow `setup-required` tile that
prompts the user to choose a subsystem. There is no default: every
camera must be explicitly assigned. (Phase 2: a "best-guess"
suggestion engine that picks Native for any camera the Protect NVR
adopts and FrigateOnly for anything else, but the user still confirms.)

---

## Event-flow contract

Both `cave-home-camera` (`CameraEvent`) and `cave-home-unifi-protect`
(`ProtectEvent`) emit events; the cave-home-automation EventBus
accepts both, but the **seam decides which one is authoritative**
for each camera.

Pseudocode (Phase 2 wiring):

```rust
// cave-home-automation::ingest::camera_event(ev)
let owner = seam.owner_of(&ev.camera_id())?;
match (owner, ev) {
    (ProtectSubsystem::Native, AnyEvent::Protect(p))   => bus.emit(p.into_camera_event()),
    (ProtectSubsystem::Native, AnyEvent::Frigate(_))   => /* silenced */,
    (ProtectSubsystem::FrigateMl, AnyEvent::Frigate(f))=> bus.emit(f.into_camera_event()),
    (ProtectSubsystem::FrigateMl, AnyEvent::Protect(_))=> /* silenced */,
    (ProtectSubsystem::FrigateOnly, AnyEvent::Frigate(f)) => bus.emit(f.into_camera_event()),
    (ProtectSubsystem::FrigateOnly, AnyEvent::Protect(_)) => /* impossible */,
}
```

The seam never silently picks; every camera has exactly one owner.

---

## Stream-URL resolution

| Subsystem      | Stream URL source                                                      |
| -------------- | ---------------------------------------------------------------------- |
| `Native`       | UniFi Protect `camera.channels[k].rtsp_alias` (proxied by the NVR).    |
| `FrigateMl`    | UniFi Protect `camera.channels[k].rtsp_alias` → Frigate ffmpeg input.  |
| `FrigateOnly`  | User-supplied RTSP URL stored in `cave_home_camera::CameraConfig::source`. |

The Portal **never** displays the raw RTSP URL by default (ADR-007 §3:
no `rtsp://...` in grandma view). Verbose mode (`?verbose=1`) shows
it.

---

## Doorbell rings

UniFi G4 Doorbell ring events arrive over the Protect WebSocket as
`EventKind::Ring`. These are **always Protect-owned** — Frigate has
no equivalent ring detection. The portal's "Ön kapı zili çaldı"
notification is wired to `ProtectEvent { kind: Ring, .. }`
unconditionally, regardless of the camera's seam assignment.

UniFi **Access** doorbell rings (separate hub) are a different
event-source (`cave_home_unifi_access::DoorEvent { kind:
DoorbellRing, .. }`) and feed the same notification surface; both
crates' events converge through the `cave_home_automation::doorbell`
sink.

---

## Where the seam lives in persistence

Phase 1: in-memory only.

Phase 2: persisted to the cave-home K8s-class config-store as a
single ConfigMap-equivalent object. The Portal's `/admin/unifi/protect/seam`
page is the editor; `cavehomectl unifi protect seam assign <camera>
<subsystem>` is the CLI path.

---

## Open questions

1. **Multi-NVR households.** What if a user has two UniFi Protect NVRs?
   Each NVR has its own bootstrap; the seam keys on `CameraId` which
   is globally unique, so the data model already works. Open: the
   Portal admin UI shows the NVR each camera lives on as a sub-label.

2. **Camera replaced.** When a user swaps a G4 for a G5, the
   `CameraId` changes. The seam loses its entry — falls back to the
   `setup-required` tile. Open: an "import old assignment" wizard
   that matches on label.

3. **Mixed inference**. A user wants Frigate to label vehicles and
   Protect to handle person detection on the same camera. Not in
   Phase 1: the seam is one subsystem per camera. Phase 2 ticket if
   demand surfaces.
