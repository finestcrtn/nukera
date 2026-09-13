import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

const Color kBg = Color(0xFF0F1724);
const Color kSurface = Color(0xFF1C2533);
const Color kBorder = Color(0xFF2D3A4F);
const Color kPrimary = Color(0xFF1D4ED8);
const Color kAccent = Color(0xFF60A5FA);
const Color kMuted = Color(0xFF8899AA);
const Color kPurple = Color(0xFF6D28D9);
const Color kRed = Color(0xFFDC2626);

enum EngineState { disconnected, connecting, connected, optimizing, failed }

enum AppLang { auto, en, ru }

String _detectSystemLang() {
  final loc = Platform.localeName.toLowerCase();
  return loc.startsWith('ru') ? 'ru' : 'en';
}

class Loc {
  static final ValueNotifier<AppLang> notifier = ValueNotifier(AppLang.en);

  static AppLang get lang => notifier.value;
  static set lang(AppLang v) => notifier.value = v;

  static String t(String key) =>
      (lang == AppLang.ru ? _ru[key] : _en[key]) ?? key;

  static const Map<String, String> _en = {
    'appTitle': 'Nukera',
    'connected': 'Connected',
    'connecting': 'Connecting...',
    'disconnecting': 'Disconnecting...',
    'optimizing': 'Optimizing...',
    'failed': 'Failed',
    'disconnected': 'Disconnected',
    'setupTelegram': 'Setup Telegram',
    'adapt': 'Adapt to your internet',
    'stopTests': 'Stop tests',
    'stopping': 'Stopping tests…',
    'preparing': 'Preparing...',
    'settings': 'Settings',
    'language': 'Language',
    'auto': 'Auto (system language)',
    'english': 'English',
    'russian': 'Русский',
    'silentAutostart': 'Silent autostart',
    'silentAutostartSub': 'Start the bypass when the system starts',
    'notFound': 'Nukera CLI not found. Place this app next to the Nukera '
        'project or set the NUKERA_ROOT environment variable.',
    'cannotReach': 'Cannot reach Nukera CLI.',
    'contactingWorker': 'Contacting worker...',
    'bypassNotRunning': 'Bypass is not running - start Nukera first.',
    'telegramOpened': 'Telegram was opened - click Connect/OK once.',
    'setupFailed': 'Setup failed. Open Nukera in a terminal for details.',
    'couldNotConnect': 'Could not connect. Run `nukera start` in a terminal to '
        'see the error, and grant admin permission.',
    'adminPassword': 'Nukera needs a one-time admin permission — check for the '
        'macOS password prompt.',
    'currentStrategy': 'Current strategy',
    'strategyHint': 'Paste ciadpi flags. Unsupported flags are dropped, the '
        'engine restarts to apply.',
    'apply': 'Apply',
    'cancel': 'Cancel',
  };

  static const Map<String, String> _ru = {
    'appTitle': 'Nukera',
    'connected': 'Подключено',
    'connecting': 'Подключение...',
    'disconnecting': 'Отключение...',
    'optimizing': 'Оптимизация...',
    'failed': 'Ошибка',
    'disconnected': 'Отключено',
    'setupTelegram': 'Настроить Telegram',
    'adapt': 'Адаптировать под ваш интернет',
    'stopTests': 'Остановить тесты',
    'stopping': 'Останавливаем тесты…',
    'preparing': 'Подготовка...',
    'settings': 'Настройки',
    'language': 'Язык',
    'auto': 'Авто (язык системы)',
    'english': 'English',
    'russian': 'Русский',
    'silentAutostart': 'Тихий автозапуск',
    'silentAutostartSub': 'Запускать обход при старте системы',
    'notFound': 'Nukera CLI не найден. Поместите приложение рядом с проектом '
        'Nukera или задайте переменную NUKERA_ROOT.',
    'cannotReach': 'Не удаётся связаться с Nukera CLI.',
    'contactingWorker': 'Связываемся с воркером...',
    'bypassNotRunning': 'Обход не запущен — сначала запустите Nukera.',
    'telegramOpened': 'Telegram открыт — нажмите Connect/OK один раз.',
    'setupFailed': 'Не удалось настроить. Откройте Nukera в терминале.',
    'couldNotConnect': 'Не удалось подключиться. Запустите `nukera start` в '
        'терминале, чтобы увидеть ошибку, и дайте права администратора.',
    'adminPassword': 'Nukera один раз запрашивает права администратора — '
        'введите пароль в окне macOS.',
    'currentStrategy': 'Текущая стратегия',
    'strategyHint': 'Вставьте флаги ciadpi. Неподдерживаемые флаги '
        'отбрасываются, движок перезапускается.',
    'apply': 'Применить',
    'cancel': 'Отмена',
  };
}

class SettingsManager {
  static AppLang lang = AppLang.auto;
  static bool silentAutostart = false;
  static String root = '';
  static String pythonPath = '';
  static bool bundled = false;
  static String _path = '';

  static String get effectiveLang {
    if (lang != AppLang.auto) return lang.name;
    return _detectSystemLang();
  }

  static Future<void> load(String root, {String python = '', bool bundled = false}) async {
    SettingsManager.root = root;
    SettingsManager.pythonPath = python;
    SettingsManager.bundled = bundled;
    _path = root.isEmpty ? '' : '$root/state/gui.settings.json';
    if (_path.isNotEmpty) {
      try {
        final f = File(_path);
        if (await f.exists()) {
          final m = jsonDecode(await f.readAsString()) as Map<String, dynamic>;
          final l = m['lang'];
          lang = l == 'en' || l == 'ru' ? AppLang.values.byName(l) : AppLang.auto;
          silentAutostart = m['silentAutostart'] == true;
        }
      } catch (_) {}
    }
    Loc.lang = effectiveLang == 'ru' ? AppLang.ru : AppLang.en;
  }

  static Future<void> _save() async {
    if (_path.isEmpty) return;
    try {
      await File(_path).writeAsString(jsonEncode({
        'lang': lang.name,
        'silentAutostart': silentAutostart,
      }));
    } catch (_) {}
  }

  static Future<void> setLang(AppLang v) async {
    lang = v;
    Loc.lang = effectiveLang == 'ru' ? AppLang.ru : AppLang.en;
    await _save();
  }

  static Future<void> setSilentAutostart(bool v, String root) async {
    silentAutostart = v;
    await _applyAutostart(v, root);
    await _save();
  }

  static const String _agentName = 'com.nukera.bypass';

  static Future<void> _applyAutostart(bool enable, String root) async {
    if (root.isEmpty) return;
    try {
      final home = Platform.environment['HOME'] ?? '';
      if (home.isEmpty) return;
      final plist = File('$home/Library/LaunchAgents/$_agentName.plist');
      final uid = (await Process.run('id', ['-u'])).stdout.toString().trim();
      if (enable) {
        await plist.parent.create(recursive: true);
        final content = '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
            '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
            '<plist version="1.0">\n<dict>\n'
            '  <key>Label</key><string>$_agentName</string>\n'
            '  <key>ProgramArguments</key><array>\n'
            '    <string>${SettingsManager.pythonPath.isNotEmpty ? SettingsManager.pythonPath : '/usr/bin/python3'}</string>\n'
            '    <string>-u</string>\n'
            '    <string>$root/cli/nukera.py</string>\n'
            '    <string>start</string>\n'
            '  </array>\n'
            '  <key>WorkingDirectory</key><string>$root</string>\n'
            '  ${SettingsManager.bundled ? '<key>EnvironmentVariables</key><dict><key>NUKERA_BUNDLED</key><string>1</string></dict>' : ''}'
            '  <key>RunAtLoad</key><true/>\n'
            '</dict>\n</plist>\n';
        await plist.writeAsString(content);
        await Process.run('launchctl', ['bootstrap', 'gui/$uid', plist.path]);
      } else {
        await Process.run('launchctl', ['bootout', 'gui/$uid', plist.path]);
        if (await plist.exists()) await plist.delete();
      }
    } catch (_) {}
  }
}

String findNukeraRoot() {
  final candidates = <String>[];
  final env = Platform.environment['NUKERA_ROOT'];
  if (env != null && env.isNotEmpty) {
    candidates.add(env);
  }
  var dir = File(Platform.resolvedExecutable).parent;
  for (var i = 0; i < 10; i++) {
    candidates.add(dir.path);
    candidates.add('${dir.path}/Nukera');
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  final home = Platform.environment['HOME'] ?? '';
  candidates.addAll([
    '$home/Documents/projects/Nukera',
    '$home/Documents/Nukera',
    '$home/projects/Nukera',
  ]);
  for (final c in candidates) {
    if (c.isNotEmpty && File('$c/cli/nukera.py').existsSync()) {
      return c;
    }
  }
  return '';
}

String? bundleCoreDir() {
  try {
    final contents = File(Platform.resolvedExecutable)
        .parent
        .parent; // MacOS -> Contents
    final res = Directory('${contents.path}/Resources');
    if (File('${res.path}/nukera-core/VERSION').existsSync()) {
      return '${res.path}/nukera-core';
    }
  } catch (_) {}
  return null;
}

String hostArch() {
  final v = Platform.version.toLowerCase();
  return (v.contains('arm64') || v.contains('aarch64')) ? 'arm64' : 'x86';
}

String bundledPython(String core) {
  return '$core/python-${hostArch()}/bin/python3';
}

void _deleteDirRecursive(String path) {
  try {
    final dir = Directory(path);
    if (dir.existsSync()) dir.deleteSync(recursive: true);
  } catch (_) {}
}

/// Self-contained install: the app ships read-only with the whole console
/// CLI + engines + strategies. First run (or version bump) re-hydrates that
/// into the user's Application Support dir where every writable path lives
/// (state/, engine/strategies/, logs). The bundled Python stays inside the
/// app bundle — it is arch-picked at launch and never copied.
Future<String> ensureAppSupportCore(String bundleCore) async {
  final home = Platform.environment['HOME'] ?? '';
  final target = '$home/Library/Application Support/Nukera/core';
  try {
    String embeddedV = '';
    final vf = File('$bundleCore/VERSION');
    if (await vf.exists()) embeddedV = (await vf.readAsString()).trim();
    String targetV = '';
    final tf = File('$target/VERSION');
    if (await tf.exists()) targetV = (await tf.readAsString()).trim();
    if (embeddedV.isNotEmpty && embeddedV == targetV) return target;
    _deleteDirRecursive(target);
    await Directory(target).create(recursive: true);
    await for (final e in Directory(bundleCore).list()) {
      final name = _seg(e.path);
      if (name.startsWith('python-')) continue;
      if (name == '.DS_Store') continue;
      final dest = '$target/$name';
      if (e is Directory) {
        await _copyDir(e.path, dest);
      } else {
        await File(e.path).copy(dest);
      }
    }
  } catch (_) {}
  return target;
}

String _seg(String p) {
  final parts = p.split(Platform.pathSeparator);
  return parts.isEmpty ? '' : parts.last;
}

Future<void> _copyDir(String src, String dst) async {
  await Directory(dst).create(recursive: true);
  await for (final e in Directory(src).list()) {
    final name = _seg(e.path);
    if (name.isEmpty) continue;
    final dest = '$dst/$name';
    if (e is Directory) {
      await _copyDir(e.path, dest);
    } else {
      await File(e.path).copy(dest);
    }
  }
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await windowManager.ensureInitialized();
  var root = findNukeraRoot();
  var python = '';
  var bundled = false;
  final bc = bundleCoreDir();
  if (bc != null) {
    root = await ensureAppSupportCore(bc);
    python = bundledPython(bc);
    bundled = true;
  }
  await SettingsManager.load(root, python: python, bundled: bundled);

  const options = WindowOptions(
    size: Size(420, 640),
    minimumSize: Size(380, 520),
    center: true,
    title: 'Nukera',
    titleBarStyle: TitleBarStyle.hidden,
    windowButtonVisibility: false,
  );
  windowManager.waitUntilReadyToShow(options, () async {
    await windowManager.show();
    await windowManager.focus();
  });

  runApp(const NukeraApp());
}

class NukeraApp extends StatelessWidget {
  const NukeraApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Nukera',
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark().copyWith(
        scaffoldBackgroundColor: kBg,
        colorScheme: ColorScheme.dark(
          primary: kPrimary,
          surface: kSurface,
        ),
        textButtonTheme: TextButtonThemeData(
          style: TextButton.styleFrom(foregroundColor: kAccent),
        ),
        progressIndicatorTheme:
            const ProgressIndicatorThemeData(color: kAccent),
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

  String _root = '';
  bool _rootOk = false;

  bool _busy = false;
  bool _adapting = false;
  bool _stopping = false;
  Process? _evalProc;
  int _adaptDone = 0;
  int _adaptTotal = 0;

  String _message = '';
  bool _messageIsError = false;

  final List<Process> _live = <Process>[];
  int _pollErrors = 0;

  Timer? _pollTimer;

  bool _disconnecting = false;
  String _strategyName = '';
  String _strategyArgs = '';
  bool _pendingConnect = false;
  DateTime? _connectDeadline;

  String get _statusText {
    if (_disconnecting) return Loc.t('disconnecting');
    switch (_state) {
      case EngineState.connected:
        return Loc.t('connected');
      case EngineState.connecting:
        return Loc.t('connecting');
      case EngineState.optimizing:
        return Loc.t('optimizing');
      case EngineState.failed:
        return Loc.t('failed');
      case EngineState.disconnected:
        return Loc.t('disconnected');
    }
  }

  @override
  void initState() {
    super.initState();
    Loc.notifier.addListener(_onLangChanged);
    _resolveRoot();
  }

  @override
  void dispose() {
    Loc.notifier.removeListener(_onLangChanged);
    _pollTimer?.cancel();
    for (final p in _live) {
      try {
        p.kill();
      } catch (_) {}
    }
    _live.clear();
    super.dispose();
  }

  void _onLangChanged() {
    if (!mounted) return;
    setState(() => _message = '');
  }

  void _resolveRoot() {
    final root = SettingsManager.root;
    if (root.isNotEmpty) {
      setState(() {
        _root = root;
        _rootOk = true;
      });
      _startPolling();
      _pollStatus();
    } else {
      setState(() {
        _messageIsError = true;
        _message = Loc.t('notFound');
      });
    }
  }

  Future<List<String>> _runCli(List<String> args) async {
    final Process p;
    try {
      p = await Process.start(
        SettingsManager.pythonPath.isNotEmpty
            ? SettingsManager.pythonPath
            : '/usr/bin/python3',
        ['-u', 'cli/nukera.py', ...args],
        workingDirectory: _root,
        environment:
            SettingsManager.bundled ? {'NUKERA_BUNDLED': '1'} : null,
      );
    } catch (_) {
      return const <String>[];
    }
    _live.add(p);
    final lines = <String>[];
    p.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(lines.add, onError: (_) {});
    p.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((_) {}, onError: (_) {});
    try {
      await p.exitCode;
    } catch (_) {}
    _live.remove(p);
    return lines;
  }

  Future<int> _runCliLive(
    List<String> args,
    void Function(String line) onLine,
  ) async {
    final Process p;
    try {
      p = await Process.start(
        SettingsManager.pythonPath.isNotEmpty
            ? SettingsManager.pythonPath
            : '/usr/bin/python3',
        ['-u', 'cli/nukera.py', ...args],
        workingDirectory: _root,
        environment:
            SettingsManager.bundled ? {'NUKERA_BUNDLED': '1'} : null,
      );
    } catch (_) {
      return 1;
    }
    _live.add(p);
    _evalProc = p;
    p.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(onLine, onError: (_) {});
    p.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(onLine, onError: (_) {});
    final code = await p.exitCode;
    _live.remove(p);
    _evalProc = null;
    return code;
  }

  void _startPolling() {
    _pollTimer = Timer.periodic(const Duration(seconds: 2), (_) {
      _pollStatus();
    });
  }

  Future<void> _pollStatus() async {
    if (_busy || _adapting || !_rootOk || !mounted) return;
    var lines = const <String>[];
    try {
      lines = await _runCli(const ['status']);
    } catch (_) {}
    if (!mounted) return;
    if (lines.isEmpty) {
      _pollErrors++;
      if (_pollErrors >= 3 && _state != EngineState.failed) {
        setState(() => _state = EngineState.failed);
        _pendingConnect = false;
        _setMessage(Loc.t('cannotReach'), isError: true);
      }
      return;
    }
    _pollErrors = 0;
    String? stratName;
    String? stratArgs;
    for (final l in lines) {
      final t = l.trim();
      if (t.startsWith('strategy_name:')) {
        stratName = t.substring('strategy_name:'.length).trim();
      } else if (t.startsWith('strategy:')) {
        stratArgs = t.substring('strategy:'.length).trim();
      }
    }
    if (stratName != null &&
        (stratName != _strategyName || stratArgs != _strategyArgs)) {
      setState(() {
        _strategyName = stratName!;
        _strategyArgs = stratArgs ?? '';
      });
    }
    if (_messageIsError && _message == Loc.t('cannotReach')) {
      _setMessage('');
    }
    final running = lines.any(
      (l) => l.trim().startsWith('running: true'),
    );
    if (running) {
      _pendingConnect = false;
      if (!_disconnecting) {
        if (_state != EngineState.connected) {
          setState(() => _state = EngineState.connected);
          _setMessage('');
        }
      }
    } else if (_pendingConnect) {
      if (_connectDeadline != null && DateTime.now().isAfter(_connectDeadline!)) {
        _pendingConnect = false;
        setState(() {
          _state = EngineState.disconnected;
          _disconnecting = false;
        });
        _setMessage(Loc.t('couldNotConnect'), isError: true);
      }
    } else {
      if (_state != EngineState.disconnected) {
        setState(() => _state = EngineState.disconnected);
      }
    }
  }

  void _setMessage(String text, {bool isError = false}) {
    setState(() {
      _message = text;
      _messageIsError = isError;
    });
  }

  Future<void> _toggleConnection() async {
    if (_busy ||
        _adapting ||
        !_rootOk ||
        _state == EngineState.connecting ||
        _state == EngineState.optimizing) {
      return;
    }
    final wantConnect = _state != EngineState.connected;
    setState(() {
      _state = EngineState.connecting;
      _disconnecting = !wantConnect;
      _message = '';
    });
    if (wantConnect) {
      _pendingConnect = true;
      _connectDeadline = DateTime.now().add(const Duration(seconds: 90));
    } else {
      _pendingConnect = false;
    }

    // Fire and forget: `start` brings the engine + system proxy up in its
    // first seconds; the rest (one-time admin-password grant, Telegram
    // proxy) may still be in flight afterwards. Never gate the UI on the
    // CLI process finishing — the 2s status poll flips us to Connected the
    // moment the engine is detected, so a pending admin prompt can no
    // longer produce a phantom 60 s spinner + false "could not connect".
    final out = <String>[];
    Process? proc;
    try {
      proc = await Process.start(
        SettingsManager.pythonPath.isNotEmpty
            ? SettingsManager.pythonPath
            : '/usr/bin/python3',
        ['-u', 'cli/nukera.py', wantConnect ? 'start' : 'stop'],
        workingDirectory: _root,
        environment:
            SettingsManager.bundled ? {'NUKERA_BUNDLED': '1'} : null,
      );
    } catch (_) {
      proc = null;
    }
    if (proc == null) {
      if (wantConnect) {
        _pendingConnect = false;
        setState(() => _state = EngineState.disconnected);
        _setMessage(Loc.t('couldNotConnect'), isError: true);
      }
      return;
    }
    _live.add(proc);
    proc.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(out.add, onError: (_) {});
    proc.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(out.add, onError: (_) {});
    _watchControl(proc, out, wantConnect);
  }

  Future<void> _watchControl(
      Process p, List<String> out, bool wantConnect) async {
    // Surface a pending admin-password prompt so the user looks for the
    // macOS dialog instead of staring at a spinner.
    await Future.delayed(const Duration(milliseconds: 1500));
    if (!mounted) return;
    if (out.any((l) =>
        l.contains('requesting elevated access') ||
        l.contains('privilege grant'))) {
      _setMessage(Loc.t('adminPassword'));
    }
    await p.exitCode;
    if (!mounted) return;
    _live.remove(p);
    final hadError = out.any((l) =>
        l.contains('error:') || l.contains('permission was not granted'));
    await _pollStatus();
    if (wantConnect && hadError && _state != EngineState.connected) {
      setState(() => _state = EngineState.disconnected);
      _setMessage(Loc.t('couldNotConnect'), isError: true);
    }
    if (!wantConnect) {
      setState(() => _disconnecting = false);
    }
  }

  Future<void> _setupTelegram() async {
    if (_busy || !_rootOk) return;
    setState(() => _message = Loc.t('contactingWorker'));
    _busy = true;
    final lines = await _runCli(const ['telegram']);
    _busy = false;
    if (!mounted) return;
    final linkLine =
        lines.where((l) => l.startsWith('TGPROXY_LINK=')).firstOrNull;
    final refused = lines.any((l) => l.contains('is not running'));
    if (refused) {
      _setMessage(Loc.t('bypassNotRunning'), isError: true);
    } else if (linkLine != null) {
      _setMessage(Loc.t('telegramOpened'));
    } else {
      _setMessage(Loc.t('setupFailed'), isError: true);
    }
  }

  void _adapt() {
    if (_adapting && _rootOk) {
      if (_stopping) return;
      setState(() {
        _stopping = true;
        _message = Loc.t('stopping');
        _messageIsError = false;
      });
      try {
        File('$_root/state/adapt.cancel').writeAsStringSync('1\n');
      } catch (_) {}
      _evalProc?.kill(ProcessSignal.sigterm);
      return;
    }
    if (_busy || !_rootOk) return;
    if (_state == EngineState.connected ||
        _state == EngineState.disconnected) {
      _runAdapt();
    }
  }

  Future<void> _runAdapt() async {
    try {
      final fc = File('$_root/state/adapt.cancel');
      if (await fc.exists()) await fc.delete();
    } catch (_) {}
    setState(() {
      _adapting = true;
      _adaptDone = 0;
      _adaptTotal = 0;
      _state = EngineState.optimizing;
      _message = '';
    });
    _busy = true;
    await _runCliLive(['eval'], (line) {
      final t = RegExp(r'^eval: \d+/(\d+) strategies').firstMatch(line);
      if (t != null) {
        setState(() => _adaptTotal = int.parse(t.group(1)!));
      }
      final p = RegExp(r'^\[\s*(\d+)/(\d+) done\]').firstMatch(line.trim());
      if (p != null) {
        final done = int.parse(p.group(1)!);
        final total = int.parse(p.group(2)!);
        if (done > 0) {
          setState(() {
            _adaptDone = done;
            _adaptTotal = total;
          });
        }
      }
    });
    _busy = false;
    if (!mounted) return;
    setState(() {
      _stopping = false;
      _adapting = false;
      _state = EngineState.disconnected;
      _message = '';
    });
    await _pollStatus();
  }

  void _openSettings() {
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => SettingsScreen(
          root: _root,
          strategyName: _strategyName,
          strategyArgs: _strategyArgs,
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBg,
      body: Column(
        children: [
          _TitleBar(),
          Expanded(
            child: Center(
              child: SingleChildScrollView(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(minHeight: 480),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      const SizedBox(height: 8),
                      _HeroButton(
                        state: _state,
                        adapting: _adapting,
                        onTap: _toggleConnection,
                      ),
                      const SizedBox(height: 24),
                      Text(
                        _statusText,
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontSize: 16,
                          color: _state == EngineState.connected
                              ? kAccent
                              : kMuted,
                        ),
                      ),
                      const SizedBox(height: 28),
                      if (_state == EngineState.connected) ...[
                        TextButton(
                          onPressed: _busy ? null : _setupTelegram,
                          child: Text(Loc.t('setupTelegram')),
                        ),
                        const SizedBox(height: 14),
                      ],
                      _AdaptButton(
                        adapting: _adapting,
                        stopping: _stopping,
                        onTap: _adapt,
                      ),
                      if (_adapting) ...[
                        const SizedBox(height: 18),
                        SizedBox(
                          width: 230,
                          child: LinearProgressIndicator(
                            value: _adaptTotal > 0
                                ? (_adaptDone / _adaptTotal).clamp(0.0, 1.0)
                                : null,
                            minHeight: 5,
                            backgroundColor: kBorder,
                            color: kAccent,
                            borderRadius: BorderRadius.circular(3),
                          ),
                        ),
                        const SizedBox(height: 8),
                        Text(
                          _adaptTotal > 0
                              ? '$_adaptDone/$_adaptTotal'
                              : Loc.t('preparing'),
                          style: const TextStyle(
                            fontSize: 13,
                            color: kMuted,
                            fontFeatures: [FontFeature.tabularFigures()],
                          ),
                        ),
                      ],
                      const SizedBox(height: 40),
                      _ActionButton(
                        label: Loc.t('settings'),
                        icon: Icons.settings,
                        onTap: _openSettings,
                      ),
                      const SizedBox(height: 12),
                      if (_message.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 28),
                          child: Text(
                            _message,
                            textAlign: TextAlign.center,
                            style: TextStyle(
                              fontSize: 12.5,
                              color: _messageIsError ? kRed : kAccent,
                            ),
                          ),
                        ),
                      const SizedBox(height: 20),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _TitleBar extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onPanStart: (_) => windowManager.startDragging(),
      child: Container(
        height: 40,
        color: kBg,
        child: Row(
          children: [
            const SizedBox(width: 14),
            Text(
              Loc.t('appTitle'),
              style: const TextStyle(
                fontWeight: FontWeight.w600,
                fontSize: 15,
              ),
            ),
            const Spacer(),
            _WindowButton(
              icon: Icons.minimize,
              onTap: () => windowManager.minimize(),
            ),
            _WindowButton(
              icon: Icons.close,
              onTap: () => windowManager.close(),
            ),
          ],
        ),
      ),
    );
  }
}

class _WindowButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback onTap;

  const _WindowButton({required this.icon, required this.onTap});

  @override
  State<_WindowButton> createState() => _WindowButtonState();
}

class _WindowButtonState extends State<_WindowButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: widget.onTap,
        child: Container(
          width: 46,
          height: 40,
          alignment: Alignment.center,
          color: _hover ? kSurface : Colors.transparent,
          child: Icon(widget.icon, size: 16, color: _hover ? Colors.white : kMuted),
        ),
      ),
    );
  }
}

class _HeroButton extends StatelessWidget {
  final EngineState state;
  final bool adapting;
  final VoidCallback onTap;

  const _HeroButton({
    required this.state,
    required this.adapting,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final busy = state == EngineState.connecting || adapting;
    Color bgColor;
    Color iconColor;
    if (adapting) {
      bgColor = kPurple;
      iconColor = Colors.white;
    } else {
      switch (state) {
        case EngineState.connected:
          bgColor = kPrimary;
          iconColor = Colors.white;
          break;
        case EngineState.connecting:
        case EngineState.optimizing:
          bgColor = kPurple;
          iconColor = Colors.white;
          break;
        case EngineState.failed:
          bgColor = kRed;
          iconColor = Colors.white;
          break;
        case EngineState.disconnected:
          bgColor = kSurface;
          iconColor = kMuted;
          break;
      }
    }

    return GestureDetector(
      onTap: busy ? null : onTap,
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
          boxShadow: state == EngineState.connected && !adapting
              ? [
                  BoxShadow(
                    color: kPrimary.withValues(alpha: 0.4),
                    blurRadius: 30,
                    spreadRadius: 5,
                  ),
                ]
              : const [],
        ),
        child: adapting
            ? const _SpinningGear(iconSize: 48)
            : state == EngineState.connecting
                ? const Padding(
                    padding: EdgeInsets.all(35),
                    child: CircularProgressIndicator(
                      color: Colors.white,
                      strokeWidth: 3,
                    ),
                  )
                : Icon(
                    state == EngineState.failed
                        ? Icons.error_outline
                        : Icons.power_settings_new,
                    size: 50,
                    color: iconColor,
                  ),
      ),
    );
  }
}

class _SpinningGear extends StatefulWidget {
  const _SpinningGear({required this.iconSize});

  final double iconSize;

  @override
  State<_SpinningGear> createState() => _SpinningGearState();
}

class _SpinningGearState extends State<_SpinningGear>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1200),
    )..repeat();
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return RotationTransition(
      turns: _controller,
      child: Icon(
        Icons.settings,
        size: widget.iconSize,
        color: Colors.white,
      ),
    );
  }
}

class _AdaptButton extends StatelessWidget {
  final bool adapting;
  final bool stopping;
  final VoidCallback onTap;

  const _AdaptButton({
    required this.adapting,
    this.stopping = false,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final active = adapting && !stopping;
    final color = stopping ? kMuted : (active ? kRed : kMuted);
    final border = active ? kRed.withValues(alpha: 0.55) : kBorder;
    return GestureDetector(
      onTap: stopping ? null : onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
        decoration: BoxDecoration(
          color: kSurface,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: border),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              stopping
                  ? Icons.hourglass_empty
                  : (adapting ? Icons.close : Icons.tune),
              size: 18,
              color: color,
            ),
            const SizedBox(width: 8),
            Text(
              adapting
                  ? (stopping ? Loc.t('stopping') : Loc.t('stopTests'))
                  : Loc.t('adapt'),
              style: TextStyle(color: color, fontSize: 14),
            ),
          ],
        ),
      ),
    );
  }
}

class _ActionButton extends StatelessWidget {
  final String label;
  final IconData icon;
  final VoidCallback onTap;

  const _ActionButton({
    required this.label,
    required this.icon,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
        decoration: BoxDecoration(
          color: kSurface,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: kBorder),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 18, color: kMuted),
            const SizedBox(width: 8),
            Text(label, style: const TextStyle(color: kMuted)),
          ],
        ),
      ),
    );
  }
}

class SettingsScreen extends StatefulWidget {
  final String root;
  final String strategyName;
  final String strategyArgs;

  const SettingsScreen(
      {super.key,
      required this.root,
      this.strategyName = '',
      this.strategyArgs = ''});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late String _name = widget.strategyName;
  late String _args = widget.strategyArgs;

  @override
  void initState() {
    super.initState();
    Loc.notifier.addListener(_onLangChanged);
  }

  @override
  void dispose() {
    Loc.notifier.removeListener(_onLangChanged);
    super.dispose();
  }

  void _onLangChanged() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBg,
      appBar: AppBar(
        title: Text(Loc.t('settings'),
            style: const TextStyle(fontWeight: FontWeight.w600)),
        backgroundColor: kBg,
        elevation: 0,
      ),
      body: ListView(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
            child: Text(
              Loc.t('language'),
              style: const TextStyle(color: kMuted, fontSize: 13),
            ),
          ),
          _LangTile(
            label: Loc.t('auto'),
            value: AppLang.auto,
          ),
          _LangTile(
            label: Loc.t('english'),
            value: AppLang.en,
          ),
          _LangTile(
            label: Loc.t('russian'),
            value: AppLang.ru,
          ),
          const Divider(height: 24, color: kBorder),
          SwitchListTile(
            activeColor: kAccent,
            title: Text(Loc.t('silentAutostart')),
            subtitle: Text(
              Loc.t('silentAutostartSub'),
              style: const TextStyle(color: kMuted, fontSize: 12.5),
            ),
            value: SettingsManager.silentAutostart,
            onChanged: (v) async {
              await SettingsManager.setSilentAutostart(v, widget.root);
              if (mounted) setState(() {});
            },
          ),
          const Divider(height: 24, color: kBorder),
          ListTile(
            title: Text(Loc.t('currentStrategy')),
            subtitle: Text(
              (_name.isEmpty ? '—' : _name) +
                  (_args.isEmpty ? '' : '\n$_args'),
              style: const TextStyle(color: kMuted, fontSize: 12.5),
            ),
            trailing:
                const Icon(Icons.edit_outlined, color: kMuted, size: 20),
            onTap: _editStrategy,
          ),
        ],
      ),
    );
  }

  Future<void> _editStrategy() async {
    final controller = TextEditingController(text: _args);
    final text = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: kSurface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Text(Loc.t('currentStrategy'),
            style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(Loc.t('strategyHint'),
                style: const TextStyle(color: kMuted, fontSize: 12.5)),
            const SizedBox(height: 12),
            TextField(
              controller: controller,
              autofocus: true,
              maxLines: 4,
              style: const TextStyle(fontSize: 12.5),
              decoration: InputDecoration(
                filled: true,
                fillColor: kBg,
                border: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(10),
                    borderSide: const BorderSide(color: kBorder)),
                hintText: '-d1 -d3+s -s6+s',
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx), child: Text(Loc.t('cancel'))),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, controller.text),
              child: Text(Loc.t('apply'))),
        ],
      ),
    );
    if (!mounted || text == null) return;
    final msg = await _applyStrategy(text);
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
      content: Text(msg, style: const TextStyle(fontSize: 13)),
      backgroundColor: msg.startsWith('error') ? kRed : kSurface,
      duration: const Duration(seconds: 4),
    ));
  }

  Future<String> _applyStrategy(String text) async {
    if (widget.root.isEmpty) return Loc.t('notFound');
    try {
      final p = await Process.start(
        '/usr/bin/python3',
        ['-u', 'cli/nukera.py', 'strategy', text],
        workingDirectory: widget.root,
      );
      final lines = <String>[];
      p.stdout
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen(lines.add, onError: (_) {});
      p.stderr
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen(lines.add, onError: (_) {});
      await p.exitCode;
      final saved =
          lines.where((l) => l.startsWith("saved as strategy 'custom':")).firstOrNull;
      if (saved != null) {
        final newArgs = saved.substring("saved as strategy 'custom':".length).trim();
        if (mounted) {
          setState(() {
            _name = 'custom';
            _args = newArgs;
          });
        }
        return saved;
      }
      if (lines.isNotEmpty) return lines.last;
      return Loc.t('setupFailed');
    } catch (_) {
      return Loc.t('setupFailed');
    }
  }
}

class _LangTile extends StatelessWidget {
  final String label;
  final AppLang value;

  const _LangTile({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    final selected = SettingsManager.lang == value;
    return ListTile(
      title: Text(label),
      trailing: selected
          ? const Icon(Icons.check, color: kAccent, size: 20)
          : null,
      onTap: () => SettingsManager.setLang(value),
    );
  }
}
