import 'dart:ffi' as ffi;
import 'dart:io' show File, Platform;
import 'package:ffi/ffi.dart' as pkgffi;

typedef _IsEnabledC = ffi.Int32 Function();
typedef _IsEnabledDart = int Function();

typedef _GetTgUrlC = ffi.Pointer<pkgffi.Utf8> Function();
typedef _GetTgUrlDart = ffi.Pointer<pkgffi.Utf8> Function();

typedef _EnableC = ffi.Pointer<pkgffi.Utf8> Function();
typedef _EnableDart = ffi.Pointer<pkgffi.Utf8> Function();

typedef _DisableC = ffi.Pointer<pkgffi.Utf8> Function();
typedef _DisableDart = ffi.Pointer<pkgffi.Utf8> Function();

typedef _FreeStringC = ffi.Void Function(ffi.Pointer<pkgffi.Utf8>);
typedef _FreeStringDart = void Function(ffi.Pointer<pkgffi.Utf8>);

typedef _SetAppPathsC = ffi.Int32 Function(
  ffi.Pointer<pkgffi.Utf8> etc,
  ffi.Pointer<pkgffi.Utf8> state,
  ffi.Pointer<pkgffi.Utf8> cache,
  ffi.Pointer<pkgffi.Utf8> config,
);
typedef _SetAppPathsDart = int Function(
  ffi.Pointer<pkgffi.Utf8> etc,
  ffi.Pointer<pkgffi.Utf8> state,
  ffi.Pointer<pkgffi.Utf8> cache,
  ffi.Pointer<pkgffi.Utf8> config,
);

typedef _RunDiagnosticsC = ffi.Pointer<pkgffi.Utf8> Function();
typedef _RunDiagnosticsDart = ffi.Pointer<pkgffi.Utf8> Function();

class NativeBridge {
  static ffi.DynamicLibrary? _lib;
  static _IsEnabledDart? _isEnabled;
  static _GetTgUrlDart? _getTgUrl;
  static _EnableDart? _enable;
  static _DisableDart? _disable;
  static _FreeStringDart? _freeString;
  static _SetAppPathsDart? _setAppPaths;
  static _RunDiagnosticsDart? _runDiagnostics;

  static bool get isAvailable => _lib != null;

  static bool _loaded = false;
  static String? _loadError;

  static void _ensureLoaded() {
    if (_loaded) return;
    _loaded = true;

    final candidates = <String>[
      ...() {
        try {
          final exe = File(Platform.resolvedExecutable).parent.path;
          return <String>[
            '$exe/lib/libunblocker_core.so',
            '$exe/unblocker_core.so',
            '$exe/../lib/unblocker-gui/lib/libunblocker_core.so',
          ];
        } catch (_) {
          return <String>[];
        }
      }(),
      '/usr/local/lib/libunblocker_core.so',
      '/usr/lib/libunblocker_core.so',
      '/opt/unblocker-gui/lib/libunblocker_core.so',
      'libunblocker_core.so',
      './libunblocker_core.so',
      'target/release/libunblocker_core.so',
      '../target/release/libunblocker_core.so',
      '../../target/release/libunblocker_core.so',
    ];

    for (final path in candidates) {
      try {
        final lib = ffi.DynamicLibrary.open(path);
        lib.lookupFunction<_IsEnabledC, _IsEnabledDart>('is_enabled');
        _lib = lib;
        _isEnabled = lib.lookupFunction<_IsEnabledC, _IsEnabledDart>('is_enabled');
        _getTgUrl = lib.lookupFunction<_GetTgUrlC, _GetTgUrlDart>('get_tg_url');
        _enable = lib.lookupFunction<_EnableC, _EnableDart>('enable');
        _disable = lib.lookupFunction<_DisableC, _DisableDart>('disable');
        _freeString = lib.lookupFunction<_FreeStringC, _FreeStringDart>('free_string');
        try {
          _setAppPaths = lib.lookupFunction<_SetAppPathsC, _SetAppPathsDart>('set_app_paths');
          _runDiagnostics = lib.lookupFunction<_RunDiagnosticsC, _RunDiagnosticsDart>('run_diagnostics');
        } catch (_) {}
        _loadError = null;
        return;
      } catch (_) {
        continue;
      }
    }
    _loadError = 'Rust lib not found (tried ${candidates.take(3).join(', ')}...)';
  }

  static bool isEnabled() {
    _ensureLoaded();
    if (_isEnabled == null) {
      try {
        return File('/tmp/unblocker.state').existsSync();
      } catch (_) {
        return false;
      }
    }
    try {
      return _isEnabled!() == 1;
    } catch (_) {
      return false;
    }
  }

  static String getTgUrl() {
    _ensureLoaded();
    if (_getTgUrl == null || _freeString == null) {
      try {
        final f = File('/etc/unblocker/tg-proxy.url');
        if (f.existsSync()) return f.readAsStringSync().trim();
      } catch (_) {}
      return '';
    }
    try {
      final ptr = _getTgUrl!();
      if (ptr == ffi.nullptr) return '';
      final s = ptr.cast<pkgffi.Utf8>().toDartString();
      _freeString!(ptr);
      return s;
    } catch (_) {
      return '';
    }
  }

  static String enable() {
    _ensureLoaded();
    if (_enable == null || _freeString == null) {
      return 'Rust lib not loaded: $_loadError';
    }
    try {
      final ptr = _enable!();
      if (ptr == ffi.nullptr) return '';
      final s = ptr.cast<pkgffi.Utf8>().toDartString();
      _freeString!(ptr);
      return s;
    } catch (e) {
      return 'FFI enable failed: $e';
    }
  }

  static String disable() {
    _ensureLoaded();
    if (_disable == null || _freeString == null) {
      return 'Rust lib not loaded: $_loadError';
    }
    try {
      final ptr = _disable!();
      if (ptr == ffi.nullptr) return '';
      final s = ptr.cast<pkgffi.Utf8>().toDartString();
      _freeString!(ptr);
      return s;
    } catch (e) {
      return 'FFI disable failed: $e';
    }
  }

  /// Run diagnostic tests and return JSON results.
  static String runDiagnostics() {
    _ensureLoaded();
    if (_runDiagnostics == null || _freeString == null) {
      return '{"error":"Rust lib not loaded: $_loadError"}';
    }
    try {
      final ptr = _runDiagnostics!();
      if (ptr == ffi.nullptr) return '{"error":"null pointer"}';
      final s = ptr.cast<pkgffi.Utf8>().toDartString();
      _freeString!(ptr);
      return s;
    } catch (e) {
      return '{"error":"$e"}';
    }
  }
}
