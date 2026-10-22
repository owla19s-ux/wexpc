import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/platform/log_service.dart';
import 'package:wex/features/settings/log_directory_panel.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('dev.wex/diagnostics');
  test('only event codes cross the shared logging boundary', () async {
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return {'authorized': false, 'saved': false, 'error': 'NOT_AUTHORIZED'};
    });
    await LogService.event('BRIDGE_READY');
    await LogService.event('token=secret');
    expect(calls.length, 1);
    expect(calls.single.arguments, {'event': 'BRIDGE_READY'});
    final status = await LogService.status();
    expect(status.saved, false);
    expect(status.error, 'NOT_AUTHORIZED');
  });
  test('platform absence is reported as unsaved rather than throwing', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
    final status = await LogService.status();
    expect(status.saved, false);
    expect(status.error, 'PLATFORM_UNAVAILABLE');
    await LogService.event('FLUTTER_ERROR');
  });
  test('exception keeps only WEX source position', () async {
    MethodCall? recorded;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      recorded = call;
      return {'authorized': false, 'saved': false};
    });
    await LogService.exception('FLUTTER_ERROR', StackTrace.fromString(
      '#0 fail (package:wex/main.dart:20:5)\nSECRET=do-not-log'));
    expect(recorded!.arguments, {'event': 'FLUTTER_ERROR', 'site': 'main.dart:20:5'});
  });
  testWidgets('automatic write failure replaces visible success', (tester) async {
    var saved = true;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async => {
      'authorized': true, 'saved': saved,
      'error': saved ? null : 'SHARED_WRITE_FAILED',
    });
    await tester.pumpWidget(const MaterialApp(home: Scaffold(body: LogDirectoryPanel())));
    await tester.pumpAndSettle();
    expect(find.text('共享日志已写入'), findsOneWidget);
    saved = false;
    await LogService.event('BRIDGE_FAILED');
    await tester.pumpAndSettle();
    expect(find.textContaining('SHARED_WRITE_FAILED'), findsOneWidget);
    expect(find.text('共享日志已写入'), findsNothing);
  });
}
