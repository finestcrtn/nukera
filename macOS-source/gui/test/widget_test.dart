import 'package:flutter_test/flutter_test.dart';

import 'package:nukera_gui/main.dart';

void main() {
  testWidgets('Nukera app smoke test', (WidgetTester tester) async {
    // Build the app and trigger a frame.
    await tester.pumpWidget(const NukeraApp());

    // App title renders.
    expect(find.text('Nukera'), findsOneWidget);
  });
}