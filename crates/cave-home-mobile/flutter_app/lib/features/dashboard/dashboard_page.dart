// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";

import "../devices/devices_page.dart";
import "../automations/automations_page.dart";
import "../scenes/scenes_page.dart";
import "../settings/settings_page.dart";

class DashboardPage extends StatefulWidget {
  const DashboardPage({super.key});

  @override
  State<DashboardPage> createState() => _DashboardPageState();
}

class _DashboardPageState extends State<DashboardPage> {
  int _idx = 0;

  static const _pages = <Widget>[
    _HomeTab(),
    DevicesPage(),
    AutomationsPage(),
    ScenesPage(),
    SettingsPage(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(child: _pages[_idx]),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _idx,
        onDestinationSelected: (i) => setState(() => _idx = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.home), label: "Ev"),
          NavigationDestination(icon: Icon(Icons.devices), label: "Cihazlar"),
          NavigationDestination(
            icon: Icon(Icons.bolt),
            label: "Otomasyon",
          ),
          NavigationDestination(icon: Icon(Icons.movie), label: "Senaryo"),
          NavigationDestination(icon: Icon(Icons.settings), label: "Ayarlar"),
        ],
      ),
    );
  }
}

class _HomeTab extends StatelessWidget {
  const _HomeTab();

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: const [
        Text(
          "cave-home",
          style: TextStyle(fontSize: 32, fontWeight: FontWeight.w700),
        ),
        SizedBox(height: 4),
        Text("Her şey yolunda."),
        SizedBox(height: 24),
        _StatusCard(label: "Hub", value: "Çalışıyor", ok: true),
        _StatusCard(label: "Cihazlar", value: "Çalışıyor", ok: true),
        _StatusCard(
          label: "Kameralar",
          value: "Bir kamera dikkat istiyor",
          ok: false,
        ),
        _StatusCard(label: "Ses asistanı", value: "Çalışıyor", ok: true),
        _StatusCard(label: "Güneş paneli", value: "Çalışıyor", ok: true),
      ],
    );
  }
}

class _StatusCard extends StatelessWidget {
  final String label;
  final String value;
  final bool ok;
  const _StatusCard({
    required this.label,
    required this.value,
    required this.ok,
  });

  @override
  Widget build(BuildContext context) {
    return Card(
      child: ListTile(
        leading: Icon(
          ok ? Icons.check_circle : Icons.warning,
          color: ok ? Colors.green : Colors.orange,
        ),
        title: Text(label),
        subtitle: Text(value),
      ),
    );
  }
}
