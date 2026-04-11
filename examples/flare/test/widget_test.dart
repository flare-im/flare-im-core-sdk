import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:flare_im/app.dart';

void main() {
  testWidgets('App smoke test', (WidgetTester tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: FlareImApp(),
      ),
    );
    await tester.pumpAndSettle();
    // [MaterialApp.title] 不会出现在 widget 树；登录页文案来自默认配置。
    expect(find.textContaining('欢迎'), findsWidgets);
  });
}
