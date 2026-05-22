// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:flutter_test/flutter_test.dart";
import "package:provider/provider.dart";

import "package:cave_home_mobile/features/devices/devices_page.dart";
import "package:cave_home_mobile/services/api_client.dart";

void main() {
  Widget wrap(Widget child, ApiClient api) {
    return MaterialApp(
      home: Provider<ApiClient>.value(value: api, child: child),
    );
  }

  testWidgets("devices list shows every seeded device", (tester) async {
    final api = MockApiClient();
    await tester.pumpWidget(wrap(const DevicesPage(), api));
    await tester.pumpAndSettle();
    expect(find.text("Salon lambası"), findsOneWidget);
    expect(find.text("Mutfak hareket sensörü"), findsOneWidget);
    expect(find.text("Ön kapı kilidi"), findsOneWidget);
    expect(find.text("Bahçe kamerası"), findsOneWidget);
  });

  testWidgets("ADR-007: device list NEVER renders the technical id",
      (tester) async {
    final api = MockApiClient();
    await tester.pumpWidget(wrap(const DevicesPage(), api));
    await tester.pumpAndSettle();
    // None of the seeded technical ids should appear anywhere on the
    // default mobile UI (ADR-007 §3 — no Developer view on mobile).
    expect(find.textContaining("0x00158d0003abcdef"), findsNothing);
    expect(find.textContaining("ZW-node-7"), findsNothing);
    expect(find.textContaining("uuid-1234-abcd"), findsNothing);
  });
}
