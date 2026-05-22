// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

class Automation {
  final String id;
  final String name;
  final bool enabled;

  const Automation({
    required this.id,
    required this.name,
    this.enabled = true,
  });

  Automation copyWith({bool? enabled}) =>
      Automation(id: id, name: name, enabled: enabled ?? this.enabled);
}
