import 'package:flutter/material.dart';
import 'home_page.dart';

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
