import 'dart:async';
import 'dart:io' show Directory, File, Platform, Process;

import 'package:flutter/material.dart';

import 'localization.dart';
import 'settings.dart';

const String tgUrlFile = '/etc/unblocker/tg-proxy.url';
const String stateFile = '/tmp/unblocker.state';

enum EngineState { disconnected, connecting, connected, optimizing, failed }

class MainScreen extends StatefulWidget {
  const MainScreen({super.key});

  @override
  State<MainScreen> createState() => _MainScreenState();
}

class _MainScreenState extends State<MainScreen> {
  EngineState _state = EngineState.disconnected;

  String get _statusText {
    switch (_state) {
      case EngineState.connected:
        return AppLocalizations.tr('connected');
      case EngineState.connecting:
        return AppLocalizations.tr('connecting');
      case EngineState.optimizing:
        return AppLocalizations.tr('optimizing');
      case EngineState.failed:
        return AppLocalizations.tr('failed');
      case EngineState.disconnected:
        return AppLocalizations.tr('disconnected');
    }
  }

  @override
  void initState() {
    super.initState();
    _checkState();
  }

  Future<void> _checkState() async {
    bool active = await File(stateFile).exists();
    if (!active) {
      final result = await Process.run('systemctl', ['is-active', 'unblocker.service']);
      active = result.stdout.toString().trim() == 'active';
    }
    if (mounted) {
      setState(() {
        _state = active ? EngineState.connected : EngineState.disconnected;
      });
    }
    // If autostart is enabled and not connected, auto-connect.
    if (!active) {
      if (await SettingsManager.isAutostartDesktopPresent()) {
        _toggleConnection();
      }
    }
  }

  Future<void> _toggleConnection() async {
    if (_state == EngineState.connecting || _state == EngineState.optimizing) return;

    if (_state == EngineState.connected) {
      // Disconnect
      setState(() => _state = EngineState.connecting);
      final result = await Process.run('systemctl', ['stop', 'unblocker.service']);
      if (result.exitCode == 0) {
        // Wait for state file to disappear.
        for (int i = 0; i < 30; i++) {
          await Future.delayed(const Duration(milliseconds: 300));
          if (!await File(stateFile).exists()) break;
        }
        if (mounted) setState(() => _state = EngineState.disconnected);
      } else {
        if (mounted) setState(() => _state = EngineState.failed);
        await Future.delayed(const Duration(seconds: 2));
        if (mounted) setState(() => _state = EngineState.disconnected);
      }
    } else {
      // Connect
      setState(() => _state = EngineState.connecting);
      final result = await Process.run('systemctl', ['start', 'unblocker.service']);
      if (result.exitCode == 0) {
        // Wait for state file to appear.
        for (int i = 0; i < 50; i++) {
          await Future.delayed(const Duration(milliseconds: 300));
          if (await File(stateFile).exists()) {
            if (mounted) setState(() => _state = EngineState.connected);
            return;
          }
        }
        if (mounted) setState(() => _state = EngineState.failed);
        await Future.delayed(const Duration(seconds: 2));
        if (mounted) setState(() => _state = EngineState.disconnected);
      } else {
        if (mounted) setState(() => _state = EngineState.failed);
        await Future.delayed(const Duration(seconds: 2));
        if (mounted) setState(() => _state = EngineState.disconnected);
      }
    }
  }

  Future<void> _setupTelegram() async {
    String url = '';
    try {
      final file = File(tgUrlFile);
      if (await file.exists()) url = (await file.readAsString()).trim();
    } catch (_) {}
    if (url.isEmpty) return;
    await Process.run('xdg-open', [url]);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(AppLocalizations.tr('appTitle'), style: const TextStyle(fontWeight: FontWeight.w600)),
        backgroundColor: const Color(0xFF0F1724),
        elevation: 0,
      ),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const SizedBox(height: 40),
            _HeroButton(
              state: _state,
              onTap: _toggleConnection,
            ),
            const SizedBox(height: 24),
            Text(
              _statusText,
              style: TextStyle(
                fontSize: 16,
                color: _state == EngineState.connected
                    ? const Color(0xFF60A5FA)
                    : const Color(0xFF8899AA),
              ),
            ),
            const SizedBox(height: 24),
            if (_state == EngineState.connected)
              TextButton(
                onPressed: _setupTelegram,
                child: Text(AppLocalizations.tr('setupTelegram'),
                    style: TextStyle(color: Color(0xFF60A5FA))),
              ),
            const SizedBox(height: 40),
            _ActionButton(
              label: AppLocalizations.tr('settings'),
              icon: Icons.settings,
              onTap: () {
                Navigator.push(
                  context,
                  MaterialPageRoute(builder: (_) => const SettingsScreen()),
                );
              },
            ),
            const SizedBox(height: 40),
          ],
        ),
      ),
    );
  }
}

class _HeroButton extends StatelessWidget {
  final EngineState state;
  final VoidCallback onTap;

  const _HeroButton({required this.state, required this.onTap});

  @override
  Widget build(BuildContext context) {
    Color bgColor;
    Color iconColor;
    IconData icon;

    switch (state) {
      case EngineState.connected:
        bgColor = const Color(0xFF1D4ED8);
        iconColor = Colors.white;
        icon = Icons.power_settings_new;
        break;
      case EngineState.connecting:
      case EngineState.optimizing:
        bgColor = const Color(0xFF6D28D9);
        iconColor = Colors.white;
        icon = Icons.hourglass_empty;
        break;
      case EngineState.failed:
        bgColor = const Color(0xFFDC2626);
        iconColor = Colors.white;
        icon = Icons.error_outline;
        break;
      case EngineState.disconnected:
        bgColor = const Color(0xFF1C2533);
        iconColor = const Color(0xFF8899AA);
        icon = Icons.power_settings_new;
        break;
    }

    return GestureDetector(
      onTap: (state == EngineState.connecting || state == EngineState.optimizing) ? null : onTap,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 300),
        width: 140,
        height: 140,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [bgColor, bgColor.withValues(alpha: 0.7)],
          ),
          boxShadow: state == EngineState.connected
              ? [
                  BoxShadow(
                    color: const Color(0xFF1D4ED8).withValues(alpha: 0.4),
                    blurRadius: 30,
                    spreadRadius: 5,
                  ),
                ]
              : [],
        ),
        child: (state == EngineState.connecting || state == EngineState.optimizing)
            ? const Padding(
                padding: EdgeInsets.all(35),
                child: CircularProgressIndicator(
                  color: Colors.white,
                  strokeWidth: 3,
                ),
              )
            : Icon(icon, size: 50, color: iconColor),
      ),
    );
  }
}

class _ActionButton extends StatelessWidget {
  final String label;
  final IconData icon;
  final VoidCallback onTap;

  const _ActionButton({required this.label, required this.icon, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
        decoration: BoxDecoration(
          color: const Color(0xFF1C2533),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: const Color(0xFF2D3A4F)),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 18, color: const Color(0xFF8899AA)),
            const SizedBox(width: 8),
            Text(label, style: const TextStyle(color: Color(0xFF8899AA))),
          ],
        ),
      ),
    );
  }
}

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool _autostart = SettingsManager.autostart;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(AppLocalizations.tr('settings')),
        backgroundColor: const Color(0xFF0F1724),
      ),
      body: ListView(
        children: [
          ListTile(
            title: Text(AppLocalizations.tr('language')),
            trailing: DropdownButton<String>(
              value: AppLocalizations.currentLocale,
              items: const [
                DropdownMenuItem(value: 'en', child: Text('English')),
                DropdownMenuItem(value: 'ru', child: Text('Русский')),
              ],
              onChanged: (String? value) async {
                if (value != null) {
                  AppLocalizations.setLocale(value);
                  await SettingsManager.setLanguage(value);
                  if (context.mounted) {
                    Navigator.of(context).pushAndRemoveUntil(
                      MaterialPageRoute(builder: (_) => const MainScreen()),
                      (route) => false,
                    );
                  }
                }
              },
            ),
          ),
          const Divider(height: 1),
          SwitchListTile(
            title: Text(AppLocalizations.tr('autostart')),
            value: _autostart,
            onChanged: (bool value) async {
              await SettingsManager.setAutostart(value);
              if (mounted) setState(() => _autostart = value);
            },
          ),
        ],
      ),
    );
  }
}
