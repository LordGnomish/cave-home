// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

/// A device the cave-home hub knows about.
///
/// ADR-007 grandma-friendly: [name], [room] and [kind] are
/// home-world vocabulary; [technicalId] is the raw identifier
/// (MAC/IEEE/EUI64) and is rendered only under Developer view —
/// which the mobile app does **not** expose at all (ADR-007 §3).
class Device {
  final String id;
  final String name;
  final String room;
  final String kind; // 'light' | 'motion' | 'lock' | 'camera' | ...
  final bool on;
  final String? technicalId;

  const Device({
    required this.id,
    required this.name,
    required this.room,
    required this.kind,
    this.on = false,
    this.technicalId,
  });

  Device copyWith({bool? on}) => Device(
        id: id,
        name: name,
        room: room,
        kind: kind,
        on: on ?? this.on,
        technicalId: technicalId,
      );

  factory Device.fromJson(Map<String, dynamic> json) => Device(
        id: json['id'] as String,
        name: json['name'] as String,
        room: json['room'] as String? ?? 'Unassigned',
        kind: json['kind'] as String? ?? 'unknown',
        on: json['on'] as bool? ?? false,
        technicalId: json['technical_id'] as String?,
      );
}
