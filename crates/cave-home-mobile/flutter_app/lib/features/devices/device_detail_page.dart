// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";

import "../../models/device.dart";

class DeviceDetailPage extends StatelessWidget {
  final Device device;
  const DeviceDetailPage({super.key, required this.device});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(device.name)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(
            device.name,
            style: const TextStyle(fontSize: 24, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 4),
          Text("Oda: ${device.room}"),
          const SizedBox(height: 4),
          Text("Tür: ${_kindFriendly(device.kind)}"),
          const SizedBox(height: 16),
          // ADR-007: do NOT show technicalId here. Mobile has no
          // Developer view toggle (ADR-007 §3).
        ],
      ),
    );
  }

  String _kindFriendly(String kind) {
    switch (kind) {
      case "light":
        return "Lamba";
      case "motion":
        return "Hareket sensörü";
      case "lock":
        return "Kilit";
      case "camera":
        return "Kamera";
      default:
        return "Cihaz";
    }
  }
}
