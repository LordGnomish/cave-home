// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "../models/automation.dart";
import "../models/device.dart";
import "../models/scene.dart";

/// Abstract Portal API client — production talks to cave-home-portal
/// (which fronts cave-home-apiserver-rs). The MVP uses [MockApiClient]
/// so the scaffold runs offline and widget tests can inject fakes.
abstract class ApiClient {
  Future<bool> login(String username, String password);
  Future<List<Device>> listDevices();
  Future<List<Automation>> listAutomations();
  Future<void> setAutomationEnabled(String id, bool enabled);
  Future<List<Scene>> listScenes();
  Future<void> triggerScene(String id);
  Future<void> toggleDevice(String id, bool on);
}

/// In-memory client used by the MVP scaffold + widget tests.
class MockApiClient implements ApiClient {
  String? _user;
  final List<Device> _devices;
  final List<Automation> _automations;
  final List<Scene> _scenes;

  MockApiClient({
    List<Device>? devices,
    List<Automation>? automations,
    List<Scene>? scenes,
  })  : _devices = devices ?? _seedDevices(),
        _automations = automations ?? _seedAutomations(),
        _scenes = scenes ?? _seedScenes();

  static List<Device> _seedDevices() => [
        const Device(
          id: "d1",
          name: "Salon lambası",
          room: "Salon",
          kind: "light",
          on: true,
          technicalId: "0x00158d0003abcdef",
        ),
        const Device(
          id: "d2",
          name: "Mutfak hareket sensörü",
          room: "Mutfak",
          kind: "motion",
          technicalId: "0x00158d0003123456",
        ),
        const Device(
          id: "d3",
          name: "Ön kapı kilidi",
          room: "Giriş",
          kind: "lock",
          technicalId: "ZW-node-7",
        ),
        const Device(
          id: "d4",
          name: "Bahçe kamerası",
          room: "Bahçe",
          kind: "camera",
          on: true,
          technicalId: "uuid-1234-abcd",
        ),
      ];

  static List<Automation> _seedAutomations() => [
        const Automation(id: "a1", name: "Akşam senaryosu", enabled: true),
        const Automation(
          id: "a2",
          name: "Kimse yoksa lambaları kapat",
          enabled: true,
        ),
        const Automation(id: "a3", name: "Tatil modu", enabled: false),
      ];

  static List<Scene> _seedScenes() => const [
        Scene(id: "s1", name: "Akşam", emoji: "🌙"),
        Scene(id: "s2", name: "Romantic Dinner", emoji: "🕯️"),
        Scene(id: "s3", name: "Movie Night", emoji: "🎬"),
        Scene(id: "s4", name: "Wake Up", emoji: "☀️"),
      ];

  @override
  Future<bool> login(String username, String password) async {
    // MVP: any non-empty pair logs in. Real auth lands in Phase 2b.
    if (username.isEmpty || password.isEmpty) return false;
    _user = username;
    return true;
  }

  @override
  Future<List<Device>> listDevices() async => List.unmodifiable(_devices);

  @override
  Future<List<Automation>> listAutomations() async =>
      List.unmodifiable(_automations);

  @override
  Future<void> setAutomationEnabled(String id, bool enabled) async {
    final idx = _automations.indexWhere((a) => a.id == id);
    if (idx < 0) return;
    _automations[idx] = _automations[idx].copyWith(enabled: enabled);
  }

  @override
  Future<List<Scene>> listScenes() async => List.unmodifiable(_scenes);

  @override
  Future<void> triggerScene(String id) async {
    // No-op for the MVP. Phase 2b: POST /scenes/{id}/trigger
  }

  @override
  Future<void> toggleDevice(String id, bool on) async {
    final idx = _devices.indexWhere((d) => d.id == id);
    if (idx < 0) return;
    _devices[idx] = _devices[idx].copyWith(on: on);
  }

  /// Internal — exposed for tests asserting login state.
  String? get currentUser => _user;
}
