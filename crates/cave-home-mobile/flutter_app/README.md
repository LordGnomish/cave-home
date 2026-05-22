# cave-home-mobile — Flutter companion app

Apache-2.0 — © 2026 cave-home contributors

This directory hosts the **Flutter** companion app that ships
alongside the cave-home back-end. Per **ADR-006 (b)** Flutter is the
recommended mobile stack and per **ADR-007** the default UI uses
home-world vocabulary only — there is no Developer-view toggle on
mobile.

## Stack

* Flutter SDK `>= 3.24.0` (record actual pinned version in CI).
* Dart `>= 3.5.0`.
* Material 3 / Material You.
* `provider` for state management (MVP).
* `local_auth` for biometric login.
* `geolocator` + `flutter_local_notifications` for geofencing.
* Push notifications are routed through the cave-home back-end
  (Charter §9 privacy-first — no third-party push relay).

## MVP feature coverage

| Feature                                    | Status |
| ------------------------------------------ | ------ |
| Login (username + password)                | done   |
| Biometric login (Face ID / fingerprint)    | done   |
| Dashboard (status cards)                   | done   |
| Device list                                | done   |
| Device detail                              | done   |
| Device toggle (light on/off)               | done   |
| Automation list                            | done   |
| Automation enable / disable                | done   |
| Scene grid                                 | done   |
| Scene trigger                              | done   |
| Geofence service (add / remove / detect)   | done   |
| Push service (back-end-routed seam)        | done   |
| Settings + logout                          | done   |
| i18n (TR / EN / DE .arb files)             | scaffolded |
| FCM / APNs wiring                          | Phase 2b |
| Real Portal API client                     | Phase 2b |

## Layout

```
flutter_app/
  pubspec.yaml
  analysis_options.yaml
  lib/
    main.dart                — entrypoint, MultiProvider wiring
    app.dart                 — MaterialApp + login/dashboard gate
    models/                  — Device, Automation, Scene
    services/                — ApiClient, AuthService, PushService,
                               GeofenceService
    features/
      login/                 — login page (creds + biometric)
      dashboard/             — bottom-nav scaffold + home status cards
      devices/               — device list + detail
      automations/           — automation list + toggle
      scenes/                — scene grid + trigger
      settings/              — settings + logout
    l10n/                    — intl_en.arb, intl_tr.arb, intl_de.arb
  test/
    widget_test.dart         — login + dashboard widget tests
    devices_test.dart        — ADR-007 invariant (no technicalId in UI)
    geofence_test.dart       — geofence unit tests
    push_test.dart           — fake push smoke test
```

## Running

```sh
cd crates/cave-home-mobile/flutter_app
flutter pub get
flutter test
flutter run            # device / emulator
```

## Phase 2b backlog

* Wire `flutter-rust-bridge` against the cave-home-binary's Portal API.
* Replace `MockApiClient` with a real REST + websocket client.
* Push: `firebase_messaging` + cave-home back-end relay (Charter §9).
* Geofence background updates: `geolocator` + platform-channel callback.
* Camera live view (Frigate WebRTC).
* OS image's "Add node" QR-pairing wizard.
