import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/platform/access_admin_host.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('dev.wex/access_admin_host');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const ce = '/data/user/0/dev.wex.wex';
  const de = '/data/user_de/0/dev.wex.wex';
  Map<String, Object?> valid() => {
        'version': 1,
        'privateRoot': '$ce/files/access-grants',
        'internalRoots': [ce, de],
      };
  Future<void> rejected(Object? value) async {
    messenger.setMockMethodCallHandler(channel, (_) async => value);
    await expectLater(
        AccessAdminHost.privateLayout(),
        throwsA(isA<AccessAdminHostException>().having(
            (e) => e.toString(),
            'safe error',
            'AccessAdminHostException(HOST_UNAVAILABLE)')));
  }
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('private layout is argument-free and roots are immutable', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'privateLayout');
      expect(call.arguments, isNull);
      return valid();
    });
    final layout = await AccessAdminHost.privateLayout();
    expect(layout.privateRoot, '$ce/files/access-grants');
    expect(layout.internalRoots, [ce, de]);
    expect(() => layout.internalRoots.add('/other'), throwsUnsupportedError);
  });

  test('single host root supports pre-device-protected storage platforms', () async {
    final value = valid()..['internalRoots'] = [ce];
    messenger.setMockMethodCallHandler(channel, (_) async => value);
    expect((await AccessAdminHost.privateLayout()).internalRoots, [ce]);
  });

  test('malformed envelopes and unknown versions fail closed', () async {
    for (final value in <Object?>[
      null, 1, [], '', {}, {'version': 1},
      valid()..['version'] = 2,
      valid()..['version'] = 1.0,
      valid()..['version'] = true,
      valid()..['extra'] = 'ignored?',
      valid()..remove('privateRoot'),
      valid()..remove('internalRoots'),
    ]) {
      await rejected(value);
    }
  });

  test('all internal roots are bounded unique absolute paths', () async {
    for (final roots in <Object?>[
      null, ce, [], [ce, ce], [ce, de, '/third'], [ce, 42],
      ['relative'], ['/'], ['/private/../root'], ['/private/./root'],
      ['/private//root'], ['/private/root/'], ['/private/\u0000/root'],
      ['/1${'字' * 1400}'],
    ]) {
      await rejected(valid()..['internalRoots'] = roots);
    }
  });

  test('authority must be the fixed leaf under the first host root', () async {
    for (final root in <Object?>[
      null, 42, 'relative/access-grants', '$ce/files/logs',
      '$ce-other/files/access-grants', '$de/files/access-grants',
      '$ce/../files/access-grants', '$ce/files//access-grants',
      '$ce/files/access-grants/', '$ce/files/\u0000access-grants',
    ]) {
      await rejected(valid()..['privateRoot'] = root);
    }
  });

  test('layout platform errors are fixed and do not expose details', () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(
          code: 'secret', message: '/private/secret', details: 'raw');
    });
    await expectLater(
        AccessAdminHost.privateLayout(),
        throwsA(isA<AccessAdminHostException>().having(
            (e) => e.toString(),
            'safe error',
            'AccessAdminHostException(HOST_UNAVAILABLE)')));
  });

  test('unavailable layout has no cache or legacy root fallback', () async {
    messenger.setMockMethodCallHandler(channel, (_) async => valid());
    await AccessAdminHost.privateLayout();
    final methods = <String>[];
    messenger.setMockMethodCallHandler(channel, (call) async {
      methods.add(call.method);
      throw MissingPluginException();
    });
    await expectLater(AccessAdminHost.privateLayout(),
        throwsA(isA<AccessAdminHostException>()));
    expect(methods, ['privateLayout']);
    messenger.setMockMethodCallHandler(channel, null);
    await expectLater(AccessAdminHost.privateLayout(),
        throwsA(isA<AccessAdminHostException>()));
  });
}
