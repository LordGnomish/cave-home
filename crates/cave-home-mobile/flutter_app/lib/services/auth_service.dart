// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/foundation.dart";

import "api_client.dart";

/// Auth state for the app. Stays minimal — Phase 2b adds token
/// rotation, biometric re-auth, multi-account.
class AuthService extends ChangeNotifier {
  final ApiClient _api;
  bool _loggedIn = false;
  String? _username;

  AuthService(this._api);

  bool get loggedIn => _loggedIn;
  String? get username => _username;

  Future<bool> login(String username, String password) async {
    final ok = await _api.login(username, password);
    if (ok) {
      _loggedIn = true;
      _username = username;
      notifyListeners();
    }
    return ok;
  }

  /// Stub for the biometric path — `local_auth` plugin is wired into
  /// production code, but unit tests can override this via subclass.
  Future<bool> loginWithBiometric() async {
    // In production: `await LocalAuthentication().authenticate(...)`.
    _loggedIn = true;
    _username = _username ?? "biometric-user";
    notifyListeners();
    return true;
  }

  void logout() {
    _loggedIn = false;
    _username = null;
    notifyListeners();
  }
}
