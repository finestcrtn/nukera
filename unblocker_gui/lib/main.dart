import 'package:flutter/material.dart';

import 'src/app.dart';
import 'src/settings.dart';
import 'src/localization.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await SettingsManager.load();
  AppLocalizations.setLocale(SettingsManager.language);
  runApp(const NukeraApp());
}
