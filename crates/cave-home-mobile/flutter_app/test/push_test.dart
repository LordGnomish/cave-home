// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter_test/flutter_test.dart";

import "package:cave_home_mobile/services/push_service.dart";

void main() {
  test("FakePushService registers and unregisters", () async {
    final p = FakePushService();
    await p.registerToken("https://hub.example", "tok-123");
    expect(p.hubUrl, "https://hub.example");
    expect(p.registeredToken, "tok-123");
    await p.unregister();
    expect(p.hubUrl, isNull);
    expect(p.registeredToken, isNull);
  });
}
