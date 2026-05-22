// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// cave-home mobile companion — entrypoint.
//
// ADR-006 (b) Flutter is the recommended mobile stack.
// ADR-007 §3 — no Developer-view toggle on mobile; default UI
// uses home-world vocabulary only.

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "app.dart";
import "services/api_client.dart";
import "services/auth_service.dart";
import "services/geofence_service.dart";
import "services/push_service.dart";

void main() {
  runApp(buildApp(
    api: MockApiClient(),
    push: FakePushService(),
  ));
}

Widget buildApp({required ApiClient api, required PushService push}) {
  final auth = AuthService(api);
  final geo = GeofenceService();
  return MultiProvider(
    providers: [
      Provider<ApiClient>.value(value: api),
      ChangeNotifierProvider<AuthService>.value(value: auth),
      ChangeNotifierProvider<GeofenceService>.value(value: geo),
      Provider<PushService>.value(value: push),
    ],
    child: const CaveHomeApp(),
  );
}
