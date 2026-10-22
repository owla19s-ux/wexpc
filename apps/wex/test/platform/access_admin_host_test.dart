import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/platform/access_admin_host.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('dev.wex/access_admin_host');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('root comes only from argument-free host call', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'privateRoot');
      expect(call.arguments, isNull);
      return '/data/user/0/dev.wex.wex/files/access-grants';
    });
    expect(await AccessAdminHost.privateRoot(),
        '/data/user/0/dev.wex.wex/files/access-grants');
  });

  test('invalid host values fail closed without leaking values', () async {
    for (final value in <Object?>[
      null, 42, {}, '', 'relative/access-grants',
      '/private/../access-grants', '/private/./access-grants',
      '/private//access-grants', '/private/access-grants/',
      '/private/logs', '/private/\u0000/access-grants',
      '/${'字' * 1400}/access-grants',
    ]) {
      messenger.setMockMethodCallHandler(channel, (_) async => value);
      await expectLater(AccessAdminHost.privateRoot(),
          throwsA(isA<AccessAdminHostException>().having(
              (e) => e.toString(), 'safe error',
              'AccessAdminHostException(HOST_UNAVAILABLE)')));
    }
  });

  test('missing platform fails with fixed code', () async {
    await expectLater(AccessAdminHost.privateRoot(),
        throwsA(isA<AccessAdminHostException>()));
  });

  test('platform exception details never escape', () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(
          code: 'secret-path', message: '/private/secret', details: 'raw');
    });
    await expectLater(AccessAdminHost.privateRoot(),
        throwsA(isA<AccessAdminHostException>().having(
            (e) => e.toString(), 'safe error',
            'AccessAdminHostException(HOST_UNAVAILABLE)')));
  });

  test('no cached fallback after host becomes unavailable', () async {
    messenger.setMockMethodCallHandler(channel,
        (_) async => '/data/user/0/dev.wex.wex/files/access-grants');
    await AccessAdminHost.privateRoot();
    messenger.setMockMethodCallHandler(channel, null);
    await expectLater(AccessAdminHost.privateRoot(),
        throwsA(isA<AccessAdminHostException>()));
  });
}
