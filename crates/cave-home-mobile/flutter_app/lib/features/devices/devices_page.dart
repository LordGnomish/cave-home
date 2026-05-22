// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "../../models/device.dart";
import "../../services/api_client.dart";
import "device_detail_page.dart";

class DevicesPage extends StatefulWidget {
  const DevicesPage({super.key});

  @override
  State<DevicesPage> createState() => _DevicesPageState();
}

class _DevicesPageState extends State<DevicesPage> {
  late Future<List<Device>> _future;

  @override
  void initState() {
    super.initState();
    _future = context.read<ApiClient>().listDevices();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text("Cihazlar")),
      body: FutureBuilder<List<Device>>(
        future: _future,
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snap.hasError || snap.data == null) {
            return Center(
              child: Text("Cihazlar yüklenemedi: ${snap.error ?? ''}"),
            );
          }
          final devices = snap.data!;
          if (devices.isEmpty) {
            return const Center(child: Text("Hiç cihaz yok."));
          }
          return ListView.builder(
            itemCount: devices.length,
            itemBuilder: (context, i) {
              final d = devices[i];
              return ListTile(
                key: Key("device-${d.id}"),
                leading: _iconFor(d.kind),
                title: Text(d.name),
                subtitle: Text(d.room),
                trailing: d.kind == "light"
                    ? Switch(
                        value: d.on,
                        onChanged: (v) async {
                          await context.read<ApiClient>().toggleDevice(d.id, v);
                          setState(() {
                            _future = context.read<ApiClient>().listDevices();
                          });
                        },
                      )
                    : const Icon(Icons.chevron_right),
                onTap: () => Navigator.of(context).push(MaterialPageRoute(
                  builder: (_) => DeviceDetailPage(device: d),
                )),
              );
            },
          );
        },
      ),
    );
  }

  Widget _iconFor(String kind) {
    switch (kind) {
      case "light":
        return const Icon(Icons.lightbulb);
      case "motion":
        return const Icon(Icons.sensors);
      case "lock":
        return const Icon(Icons.lock);
      case "camera":
        return const Icon(Icons.videocam);
      default:
        return const Icon(Icons.device_unknown);
    }
  }
}
