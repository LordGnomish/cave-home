// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter_test/flutter_test.dart";

import "package:cave_home_mobile/services/geofence_service.dart";

void main() {
  group("GeofenceService", () {
    test("addZone / removeZone / zones", () {
      final g = GeofenceService();
      g.addZone(Geofence(
        id: "home",
        name: "Ev",
        latitude: 49.706,
        longitude: 10.265,
        radiusMeters: 50,
      ));
      expect(g.zones, hasLength(1));
      g.removeZone("home");
      expect(g.zones, isEmpty);
    });

    test("isInside returns true when point is inside the radius", () {
      final g = GeofenceService();
      final z = Geofence(
        id: "home",
        name: "Ev",
        latitude: 49.706,
        longitude: 10.265,
        radiusMeters: 500, // half a kilometre
      );
      g.addZone(z);
      // ~10 metres east of the centre.
      expect(g.isInside(z, 49.706, 10.265 + 0.0001), isTrue);
    });

    test("isInside returns false when far away", () {
      final g = GeofenceService();
      final z = Geofence(
        id: "home",
        name: "Ev",
        latitude: 49.706,
        longitude: 10.265,
        radiusMeters: 100,
      );
      g.addZone(z);
      // ~111 km away.
      expect(g.isInside(z, 50.706, 10.265), isFalse);
    });

    test("onLocationUpdate flips inside state", () {
      final g = GeofenceService();
      final z = Geofence(
        id: "home",
        name: "Ev",
        latitude: 49.706,
        longitude: 10.265,
        radiusMeters: 100,
      );
      g.addZone(z);
      expect(z.inside, isFalse);
      g.onLocationUpdate(49.706, 10.265);
      expect(z.inside, isTrue);
      g.onLocationUpdate(50.706, 10.265);
      expect(z.inside, isFalse);
    });
  });
}
