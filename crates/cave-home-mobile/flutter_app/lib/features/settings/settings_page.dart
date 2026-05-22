// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "../../services/auth_service.dart";

class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context) {
    final auth = context.watch<AuthService>();
    return Scaffold(
      appBar: AppBar(title: const Text("Ayarlar")),
      body: ListView(
        children: [
          ListTile(
            leading: const Icon(Icons.person),
            title: const Text("Hesabım"),
            subtitle: Text(auth.username ?? "—"),
          ),
          const ListTile(
            leading: Icon(Icons.notifications),
            title: Text("Bildirimler"),
            subtitle: Text("Açık"),
          ),
          const ListTile(
            leading: Icon(Icons.location_on),
            title: Text("Konum / geofence"),
            subtitle: Text("Açık"),
          ),
          const ListTile(
            leading: Icon(Icons.language),
            title: Text("Dil"),
            subtitle: Text("Türkçe / English / Deutsch (ADR-007 M1)"),
          ),
          const Divider(),
          ListTile(
            leading: const Icon(Icons.logout),
            title: const Text("Çıkış yap"),
            onTap: () => context.read<AuthService>().logout(),
          ),
          const SizedBox(height: 24),
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: 16),
            child: Text(
              // ADR-007 §3: Developer-view toggle is intentionally absent
              // on mobile.
              "Geliştirici görünümü yalnızca masaüstü Portal'da bulunur.",
              style: TextStyle(color: Colors.grey),
            ),
          ),
        ],
      ),
    );
  }
}
