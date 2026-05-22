// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/foundation.dart";

/// Owntracks-class geofencing.
///
/// Charter §3 lists location tracking as a first-class pillar.
/// MVP exposes a region-add / region-list / region-state surface;
/// Phase 2b wires `geolocator` background updates and back-end
/// upload (privacy-first: home-only by default).
class Geofence {
  final String id;
  final String name;
  final double latitude;
  final double longitude;
  final double radiusMeters;
  bool inside;

  Geofence({
    required this.id,
    required this.name,
    required this.latitude,
    required this.longitude,
    required this.radiusMeters,
    this.inside = false,
  });
}

class GeofenceService extends ChangeNotifier {
  final List<Geofence> _zones = [];

  List<Geofence> get zones => List.unmodifiable(_zones);

  void addZone(Geofence z) {
    _zones.add(z);
    notifyListeners();
  }

  void removeZone(String id) {
    _zones.removeWhere((z) => z.id == id);
    notifyListeners();
  }

  /// Pure helper, easy to unit-test.
  bool isInside(Geofence z, double lat, double lng) {
    final dLat = z.latitude - lat;
    final dLng = z.longitude - lng;
    // Cheap planar approximation — for the MVP scaffold tests only.
    // Phase 2b switches to haversine.
    final dist = (dLat * dLat + dLng * dLng);
    final rDeg = z.radiusMeters / 111_320.0; // metres per deg at equator
    return dist <= (rDeg * rDeg);
  }

  /// Called by the platform-channel callback with the latest fix.
  void onLocationUpdate(double lat, double lng) {
    var changed = false;
    for (final z in _zones) {
      final now = isInside(z, lat, lng);
      if (now != z.inside) {
        z.inside = now;
        changed = true;
      }
    }
    if (changed) notifyListeners();
  }
}
