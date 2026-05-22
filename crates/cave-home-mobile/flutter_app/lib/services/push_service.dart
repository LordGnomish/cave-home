// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

/// Push-notification routing.
///
/// Charter §9 privacy-first: cave-home does **not** register the
/// device directly with Firebase / APNs. Instead:
///   1. The OS-level FCM / APNs token is sent to the cave-home
///      back-end.
///   2. The back-end relays notifications through FCM / APNs using
///      its own (rotatable, audit-logged) credentials.
///
/// This file is the Dart-side seam — Phase 2b wires
/// `firebase_messaging` + `flutter_apns` here. The MVP exposes the
/// public surface and a fake implementation for tests.
abstract class PushService {
  /// Register the current OS token with the cave-home hub.
  Future<void> registerToken(String hubUrl, String token);

  /// Cancel registration on logout.
  Future<void> unregister();
}

class FakePushService implements PushService {
  String? registeredToken;
  String? hubUrl;

  @override
  Future<void> registerToken(String hubUrl, String token) async {
    this.hubUrl = hubUrl;
    registeredToken = token;
  }

  @override
  Future<void> unregister() async {
    registeredToken = null;
    hubUrl = null;
  }
}
