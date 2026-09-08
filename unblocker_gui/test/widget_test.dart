// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:unblocker_gui/src/app.dart';

void main() {
  testWidgets('UnblockerApp loads correctly', (WidgetTester tester) async {
    await tester.pumpWidget(const UnblockerApp());
    await tester.pumpAndSettle(const Duration(seconds: 2));
    // Should have one toggle button (either Enable or Disable) and scaffold
    expect(find.byType(ElevatedButton), findsOneWidget);
    expect(find.byType(Scaffold), findsOneWidget);
    // Check that either Enable or Disable is shown (depending on /tmp/unblocker.state)
    final enable = find.text('▶ Enable').evaluate().length;
    final disable = find.text('■ Disable').evaluate().length;
    expect(enable + disable, 1);
  });
}
