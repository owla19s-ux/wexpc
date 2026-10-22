import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/platform/storage_access_service.dart';
import 'package:wex/features/settings/storage_access_panel.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('dev.wex/storage_access');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('missing or contradictory platform fields never claim authorization',
      () async {
    for (final value in <Map<String, Object?>>[
      {},
      {'supported': true},
      {'supported': false, 'granted': true},
      {'supported': true, 'granted': 'true'},
      {'supported': true, 'granted': true, 'error': 'CHECK_FAILED'},
    ]) {
      messenger.setMockMethodCallHandler(channel, (_) async => value);
      expect((await StorageAccessService.status()).granted, false);
    }
  });

  test('platform absence reports unavailable, never granted', () async {
    final state = await StorageAccessService.status();
    expect(state.granted, false);
    expect(state.error, 'PLATFORM_UNAVAILABLE');
    expect(await StorageAccessService.openSettings(), false);
  });

  testWidgets('opening settings is not proof of granting permission',
      (tester) async {
    final calls = <String>[];
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call.method);
      if (call.method == 'openSettings') return true;
      return {'supported': true, 'granted': false};
    });
    await tester.pumpWidget(
        const MaterialApp(home: Scaffold(body: StorageAccessPanel())));
    await tester.pumpAndSettle();
    expect(find.text('所有文件访问：未授权'), findsOneWidget);
    await tester.tap(find.text('打开系统授权设置'));
    await tester.pumpAndSettle();
    expect(calls.where((method) => method == 'openSettings').length, 1);
    expect(find.text('所有文件访问：未授权'), findsOneWidget);
    expect(find.text('所有文件访问：已授权'), findsNothing);
    expect(find.textContaining('不会自动授予 AI'), findsOneWidget);
  });

  testWidgets('resume rechecks real state and observes revocation',
      (tester) async {
    var granted = true;
    messenger.setMockMethodCallHandler(channel, (_) async =>
        {'supported': true, 'granted': granted});
    await tester.pumpWidget(
        const MaterialApp(home: Scaffold(body: StorageAccessPanel())));
    await tester.pumpAndSettle();
    expect(find.text('所有文件访问：已授权'), findsOneWidget);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    granted = false;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(find.text('所有文件访问：未授权'), findsOneWidget);
    expect(find.text('所有文件访问：已授权'), findsNothing);
  });

  testWidgets('settings failure is visible and controls recover',
      (tester) async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      if (call.method == 'openSettings') {
        throw PlatformException(code: 'SETTINGS_UNAVAILABLE');
      }
      return {'supported': true, 'granted': false};
    });
    await tester.pumpWidget(
        const MaterialApp(home: Scaffold(body: StorageAccessPanel())));
    await tester.pumpAndSettle();
    await tester.tap(find.text('打开系统授权设置'));
    await tester.pumpAndSettle();
    expect(find.text('无法打开系统设置，请刷新或从系统设置手动进入。'), findsOneWidget);
    expect(tester.takeException(), isNull);
    expect(tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
        isNotNull);
  });
}
