import 'package:flutter/material.dart';

class AppLocalizations {
  static const Map<String, Map<String, String>> _translations = {
    'en': {
      'appTitle': 'Nukera',
      'connected': 'Connected',
      'disconnected': 'Disconnected',
      'connecting': 'Connecting...',
      'optimizing': 'Optimizing...',
      'failed': 'Failed',
      'setupTelegram': 'Setup Telegram',
      'settings': 'Settings',
      'language': 'Language',
      'autostart': 'Autostart with system',
    },
    'ru': {
      'appTitle': 'Nukera',
      'connected': 'Подключено',
      'disconnected': 'Отключено',
      'connecting': 'Подключение...',
      'optimizing': 'Оптимизация...',
      'failed': 'Ошибка',
      'setupTelegram': 'Настроить Telegram',
      'settings': 'Настройки',
      'language': 'Язык',
      'autostart': 'Автозапуск с системой',
    },
  };

  static String _currentLocale = 'en';

  static String get currentLocale => _currentLocale;

  static void setLocale(String locale) {
    if (_translations.containsKey(locale)) {
      _currentLocale = locale;
    }
  }

  static String tr(String key) {
    return _translations[_currentLocale]?[key] ?? _translations['en']?[key] ?? key;
  }
}
