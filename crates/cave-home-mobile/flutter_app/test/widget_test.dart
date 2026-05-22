// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:flutter_test/flutter_test.dart";

import "package:cave_home_mobile/main.dart" as app;
import "package:cave_home_mobile/services/api_client.dart";
import "package:cave_home_mobile/services/push_service.dart";

void main() {
  testWidgets("shows the login page when not authenticated",
      (tester) async {
    await tester.pumpWidget(app.buildApp(
      api: MockApiClient(),
      push: FakePushService(),
    ));
    expect(find.text("Eve hoş geldin"), findsOneWidget);
    expect(find.byKey(const Key("login-submit")), findsOneWidget);
    expect(find.byKey(const Key("login-biometric")), findsOneWidget);
  });

  testWidgets("login flow drops the user into the dashboard",
      (tester) async {
    await tester.pumpWidget(app.buildApp(
      api: MockApiClient(),
      push: FakePushService(),
    ));
    await tester.enterText(find.byType(TextField).first, "ali");
    await tester.enterText(find.byType(TextField).last, "secret");
    await tester.tap(find.byKey(const Key("login-submit")));
    await tester.pumpAndSettle();
    expect(find.text("cave-home"), findsWidgets);
    expect(find.text("Cihazlar"), findsOneWidget); // nav-bar tab
  });

  testWidgets("biometric button logs the user in", (tester) async {
    await tester.pumpWidget(app.buildApp(
      api: MockApiClient(),
      push: FakePushService(),
    ));
    await tester.tap(find.byKey(const Key("login-biometric")));
    await tester.pumpAndSettle();
    expect(find.text("Cihazlar"), findsOneWidget);
  });

  testWidgets("login fails on empty creds", (tester) async {
    await tester.pumpWidget(app.buildApp(
      api: MockApiClient(),
      push: FakePushService(),
    ));
    await tester.tap(find.byKey(const Key("login-submit")));
    await tester.pumpAndSettle();
    expect(find.textContaining("doğru değil"), findsOneWidget);
  });
}
