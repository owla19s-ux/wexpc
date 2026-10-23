import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/main.dart';

void main() {
  testWidgets('three navigation destinations exist and space is initial', (tester) async {
    await tester.pumpWidget(const WexApp());
    expect(find.byType(NavigationDestination), findsNWidgets(3));
    expect(find.text('暂无空间'), findsOneWidget);
    expect(find.text('空间'), findsWidgets);
    expect(find.text('对话'), findsWidgets);
    expect(find.text('设置'), findsWidgets);
  });

  testWidgets('conversation draft survives navigation without claiming send', (tester) async {
    await tester.pumpWidget(const WexApp());
    await tester.tap(find.byIcon(Icons.chat_bubble_outline));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('message-draft')), '你好，WEX');
    expect(tester.widget<IconButton>(find.byKey(const ValueKey('send-message'))).onPressed, isNull);
    await tester.tap(find.byIcon(Icons.settings_outlined));
    await tester.pumpAndSettle();
    expect(find.text('连接与服务'), findsOneWidget);
    await tester.tap(find.byIcon(Icons.chat_bubble_outline));
    await tester.pumpAndSettle();
    expect(find.text('你好，WEX'), findsOneWidget);
    expect(find.text('尚未连接 AI'), findsOneWidget);
  });

  testWidgets('android back returns to space and keyboard does not overflow', (tester) async {
    tester.view.physicalSize = const Size(360, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetViewInsets);
    await tester.pumpWidget(const WexApp());
    await tester.tap(find.byIcon(Icons.chat_bubble_outline));
    await tester.pumpAndSettle();
    tester.view.viewInsets = const FakeViewPadding(bottom: 280);
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    tester.view.resetViewInsets();
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(find.text('暂无空间'), findsOneWidget);
  });
}

