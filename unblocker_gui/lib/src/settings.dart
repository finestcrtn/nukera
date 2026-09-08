import 'dart:convert';
import 'dart:io';

class SettingsManager {
  static const _fileName = 'settings.json';
  static String get _filePath =>
      '${Platform.environment['HOME']}/.config/unblocker/$_fileName';

  static String _language = 'en';
  static bool _autostart = false;

  static String get language => _language;
  static bool get autostart => _autostart;

  static Future<void> load() async {
    try {
      final file = File(_filePath);
      if (await file.exists()) {
        final content = await file.readAsString();
        final map = Map<String, dynamic>.from(
            const JsonDecoder().convert(content) as Map);
        _language = map['language'] as String? ?? 'en';
        _autostart = map['autostart'] as bool? ?? false;
      }
    } catch (_) {}
  }

  static Future<void> save() async {
    final dir = Directory('${Platform.environment['HOME']}/.config/unblocker');
    await dir.create(recursive: true);
    final file = File(_filePath);
    final data = {
      'language': _language,
      'autostart': _autostart,
    };
    await file.writeAsString(
        const JsonEncoder().convert(data));
  }

  static Future<void> setLanguage(String lang) async {
    _language = lang;
    await save();
  }

  static Future<void> setAutostart(bool value) async {
    _autostart = value;
    await _updateAutostartDesktop(value);
    await save();
  }

  static Future<void> _updateAutostartDesktop(bool enabled) async {
    final autostartDir =
        Directory('${Platform.environment['HOME']}/.config/autostart');
    final desktopFile = File('${autostartDir.path}/unblocker.desktop');

    if (enabled) {
      await autostartDir.create(recursive: true);
      await desktopFile.writeAsString('''[Desktop Entry]
Type=Application
Name=Unblocker
Exec=/opt/unblocker-gui/unblocker-gui
Hidden=false
NoDisplay=false
X-GNOME-Autostart-enabled=true
''');
    } else {
      if (await desktopFile.exists()) {
        await desktopFile.delete();
      }
    }
  }

  /// Check if autostart .desktop file exists (for launch auto-connect).
  static Future<bool> isAutostartDesktopPresent() async {
    final desktopFile = File(
        '${Platform.environment['HOME']}/.config/autostart/unblocker.desktop');
    return desktopFile.exists();
  }
}
