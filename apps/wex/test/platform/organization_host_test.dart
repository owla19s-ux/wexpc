import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/platform/organization_host.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('dev.wex/organization_host');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('private root comes exclusively from an argument-free trusted call', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'privateRoot');
      expect(call.arguments, isNull);
      return '/data/user/0/dev.wex.wex/files/organization-state';
    });
    expect(await OrganizationHost.privateRoot(),
        '/data/user/0/dev.wex.wex/files/organization-state');
  });

  test('invalid roots reject without path leakage', () async {
    for (final bad in <Object?>[
      null, 3, {}, '', 'relative/files/organization-state',
      '/private/../files/organization-state',
      '/private/./files/organization-state',
      '/private//files/organization-state',
      '/private/files/organization-state/',
      '/private/files/access-grants',
      '/private/files/\u0000organization-state',
      '/1${'字' * 1400}/files/organization-state',
    ]) {
      messenger.setMockMethodCallHandler(channel, (_) async => bad);
      await expectLater(OrganizationHost.privateRoot(),
          throwsA(isA<OrganizationHostException>().having(
              (e) => e.toString(), 'safe error',
              'OrganizationHostException(HOST_UNAVAILABLE)')));
    }
  });

  test('host exceptions and absent host have no fallback', () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: 'private', message: '/secret/root');
    });
    await expectLater(
        OrganizationHost.privateRoot(), throwsA(isA<OrganizationHostException>()));
    messenger.setMockMethodCallHandler(channel, null);
    await expectLater(
        OrganizationHost.privateRoot(), throwsA(isA<OrganizationHostException>()));
  });
}
