// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "features/dashboard/dashboard_page.dart";
import "features/login/login_page.dart";
import "services/auth_service.dart";

class CaveHomeApp extends StatelessWidget {
  const CaveHomeApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: "cave-home",
      theme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.teal),
      ),
      home: const _RootGate(),
      debugShowCheckedModeBanner: false,
    );
  }
}

class _RootGate extends StatelessWidget {
  const _RootGate();

  @override
  Widget build(BuildContext context) {
    final auth = context.watch<AuthService>();
    return auth.loggedIn ? const DashboardPage() : const LoginPage();
  }
}
