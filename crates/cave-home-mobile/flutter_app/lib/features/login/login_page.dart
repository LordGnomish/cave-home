// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

import "package:flutter/material.dart";
import "package:provider/provider.dart";

import "../../services/auth_service.dart";

class LoginPage extends StatefulWidget {
  const LoginPage({super.key});

  @override
  State<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends State<LoginPage> {
  final _userCtl = TextEditingController();
  final _passCtl = TextEditingController();
  bool _busy = false;
  String? _error;

  Future<void> _submit() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final auth = context.read<AuthService>();
    final ok = await auth.login(_userCtl.text, _passCtl.text);
    if (!mounted) return;
    setState(() => _busy = false);
    if (!ok) {
      setState(() => _error = "Adın veya şifren doğru değil.");
    }
  }

  Future<void> _biometric() async {
    final auth = context.read<AuthService>();
    await auth.loginWithBiometric();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text("cave-home")),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Text(
              "Eve hoş geldin",
              style: TextStyle(fontSize: 28, fontWeight: FontWeight.w600),
            ),
            const SizedBox(height: 24),
            TextField(
              controller: _userCtl,
              decoration: const InputDecoration(labelText: "Kullanıcı adı"),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _passCtl,
              obscureText: true,
              decoration: const InputDecoration(labelText: "Şifre"),
            ),
            if (_error != null) ...[
              const SizedBox(height: 8),
              Text(_error!, style: const TextStyle(color: Colors.red)),
            ],
            const SizedBox(height: 24),
            FilledButton(
              key: const Key("login-submit"),
              onPressed: _busy ? null : _submit,
              child: Text(_busy ? "Giriş yapılıyor…" : "Giriş yap"),
            ),
            const SizedBox(height: 8),
            OutlinedButton.icon(
              key: const Key("login-biometric"),
              onPressed: _biometric,
              icon: const Icon(Icons.fingerprint),
              label: const Text("Parmak izi / Face ID"),
            ),
          ],
        ),
      ),
    );
  }
}
