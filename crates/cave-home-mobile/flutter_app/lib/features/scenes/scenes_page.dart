// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "../../models/scene.dart";
import "../../services/api_client.dart";

class ScenesPage extends StatefulWidget {
  const ScenesPage({super.key});

  @override
  State<ScenesPage> createState() => _ScenesPageState();
}

class _ScenesPageState extends State<ScenesPage> {
  late Future<List<Scene>> _future;

  @override
  void initState() {
    super.initState();
    _future = context.read<ApiClient>().listScenes();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text("Senaryolar")),
      body: FutureBuilder<List<Scene>>(
        future: _future,
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          final items = snap.data ?? const <Scene>[];
          return GridView.builder(
            padding: const EdgeInsets.all(16),
            gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
              crossAxisCount: 2,
              childAspectRatio: 1.2,
              crossAxisSpacing: 12,
              mainAxisSpacing: 12,
            ),
            itemCount: items.length,
            itemBuilder: (context, i) {
              final s = items[i];
              return Card(
                key: Key("scene-${s.id}"),
                child: InkWell(
                  onTap: () async {
                    await context.read<ApiClient>().triggerScene(s.id);
                    if (context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(content: Text("${s.name} senaryosu çalıştı")),
                      );
                    }
                  },
                  child: Center(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Text(s.emoji ?? "🎬",
                            style: const TextStyle(fontSize: 36)),
                        const SizedBox(height: 8),
                        Text(s.name, textAlign: TextAlign.center),
                      ],
                    ),
                  ),
                ),
              );
            },
          );
        },
      ),
    );
  }
}
