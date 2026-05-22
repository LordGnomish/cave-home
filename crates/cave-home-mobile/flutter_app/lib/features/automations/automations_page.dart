// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "../../models/automation.dart";
import "../../services/api_client.dart";

class AutomationsPage extends StatefulWidget {
  const AutomationsPage({super.key});

  @override
  State<AutomationsPage> createState() => _AutomationsPageState();
}

class _AutomationsPageState extends State<AutomationsPage> {
  late Future<List<Automation>> _future;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  void _reload() {
    _future = context.read<ApiClient>().listAutomations();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text("Otomasyonlar")),
      body: FutureBuilder<List<Automation>>(
        future: _future,
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snap.hasError || snap.data == null) {
            return Center(
              child: Text("Otomasyonlar yüklenemedi: ${snap.error ?? ''}"),
            );
          }
          final items = snap.data!;
          if (items.isEmpty) {
            return const Center(child: Text("Henüz otomasyon yok."));
          }
          return ListView.builder(
            itemCount: items.length,
            itemBuilder: (context, i) {
              final a = items[i];
              return SwitchListTile(
                key: Key("automation-${a.id}"),
                title: Text(a.name),
                value: a.enabled,
                onChanged: (v) async {
                  await context
                      .read<ApiClient>()
                      .setAutomationEnabled(a.id, v);
                  setState(_reload);
                },
              );
            },
          );
        },
      ),
    );
  }
}
