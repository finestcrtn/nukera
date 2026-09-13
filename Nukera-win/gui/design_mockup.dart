import 'dart:async';
import 'package:flutter/material.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const NukeraApp());
}

enum EngineState { disconnected, connecting, connected, optimizing, failed }

class NukeraApp extends StatelessWidget {
  const NukeraApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Nukera',
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark().copyWith(
        scaffoldBackgroundColor: const Color(0xFF0F1724),
        colorScheme: ColorScheme.dark(
          primary: const Color(0xFF1D4ED8),
          surface: const Color(0xFF1C2533),
        ),
      ),
      home: const MainScreen(),
    );
  }
}

class MainScreen extends StatefulWidget {
  const MainScreen({super.key});

  @override
  State<MainScreen> createState() => _MainScreenState();
}

class _MainScreenState extends State<MainScreen> {
  EngineState _state = EngineState.disconnected;
  bool _autoStart = false;
  bool _autoConnect = false;

  String get _statusText {
    switch (_state) {
      case EngineState.connected:
        return 'Connected';
      case EngineState.connecting:
        return 'Connecting...';
      case EngineState.optimizing:
        return 'Optimizing...';
      case EngineState.failed:
        return 'Failed';
      case EngineState.disconnected:
        return 'Disconnected';
    }
  }

  Future<void> _toggleConnection() async {
    if (_state == EngineState.connecting || _state == EngineState.optimizing) return;

    if (_state == EngineState.connected) {
      setState(() {
        _state = EngineState.connecting;
        _statusText;
      });
      await Future.delayed(const Duration(seconds: 1));
      setState(() => _state = EngineState.disconnected);
    } else {
      setState(() => _state = EngineState.connecting);
      await Future.delayed(const Duration(seconds: 2));
      setState(() => _state = EngineState.connected);
    }
  }

  Future<void> _optimize() async {
    if (_state != EngineState.connected) return;
    setState(() => _state = EngineState.optimizing);
    await Future.delayed(const Duration(seconds: 2));
    setState(() => _state = EngineState.connected);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Nukera', style: TextStyle(fontWeight: FontWeight.w600)),
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
                onPressed: () {},
                child: const Text('Setup Telegram',
                    style: TextStyle(color: Color(0xFF60A5FA))),
              ),
            const SizedBox(height: 40),
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                _ActionButton(
                  label: 'Optimize',
                  icon: Icons.tune,
                  onTap: _optimize,
                ),
                const SizedBox(width: 16),
                _ActionButton(
                  label: 'Settings',
                  icon: Icons.settings,
                  onTap: () {
                    Navigator.push(
                      context,
                      MaterialPageRoute(
                        builder: (_) => SettingsScreen(
                          autoStart: _autoStart,
                          autoConnect: _autoConnect,
                          onAutoStartChanged: (v) => setState(() => _autoStart = v),
                          onAutoConnectChanged: (v) => setState(() => _autoConnect = v),
                        ),
                      ),
                    );
                  },
                ),
              ],
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

class SettingsScreen extends StatelessWidget {
  final bool autoStart;
  final bool autoConnect;
  final ValueChanged<bool> onAutoStartChanged;
  final ValueChanged<bool> onAutoConnectChanged;

  const SettingsScreen({
    super.key,
    required this.autoStart,
    required this.autoConnect,
    required this.onAutoStartChanged,
    required this.onAutoConnectChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings'),
        backgroundColor: const Color(0xFF0F1724),
      ),
      body: ListView(
        children: [
          ListTile(
            title: const Text('Language'),
            subtitle: const Text('System'),
            trailing: const Icon(Icons.chevron_right),
            onTap: () {},
          ),
          const Divider(height: 1),
          SwitchListTile(
            title: const Text('Auto-start on boot'),
            value: autoStart,
            onChanged: onAutoStartChanged,
          ),
          SwitchListTile(
            title: const Text('Auto-connect when opened'),
            value: autoConnect,
            onChanged: onAutoConnectChanged,
          ),
        ],
      ),
    );
  }
}
