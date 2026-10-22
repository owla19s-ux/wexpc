import 'dart:ffi';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/access_admin/access_admin_client.dart';
import 'package:wex/access_admin/access_admin_models.dart';

Matcher error(String code, {bool unknown = false}) =>
    isA<AccessAdminException>()
      .having((e) => e.code, 'code', code)
      .having((e) => e.outcomeUnknown, 'unknown', unknown);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final library = Platform.environment['WEX_TEST_LIBRARY']!;
  final fixtures = Platform.environment['WEX_TEST_FIXTURES']!;
  late Directory parent;
  late String root;
  setUp(() async {
    parent = await Directory.systemTemp.createTemp('wex-admin-client-');
    root = '${parent.path}/private-授权';
  });
  tearDown(() async { await parent.delete(recursive: true); });
  AccessAdminClient client({String? path}) =>
      AccessAdminClient.forHost(privateRoot: root, libraryPath: path ?? library);

  test('worker persists real Rust grant then conflicts and revokes after reopen',
      () async {
    final first = client();
    expect(Directory(root).existsSync(), false);
    final initial = await first.invoke(AccessAdminRequest.list(requestId: 'r1'));
    expect(initial.revision, '0');
    expect(initial.grants, isEmpty);
    expect((await first.invoke(AccessAdminRequest.set(
      requestId: 'save', expectedRevision: '0', directoryId: 'docs',
      access: DirectoryAccess.readWrite, explicitPersonal: true,
    ))).revision, '1');
    await first.close();
    final reopened = client();
    addTearDown(reopened.close);
    final loaded = await reopened.invoke(
        AccessAdminRequest.list(requestId: 'read'));
    expect(loaded.grants!.single.directoryId, 'docs');
    expect(loaded.grants!.single.explicitPersonal, true);
    await expectLater(reopened.invoke(AccessAdminRequest.revoke(
        requestId: 'stale', expectedRevision: '0', directoryId: 'docs')),
        throwsA(error('REVISION_CONFLICT')));
    expect((await reopened.invoke(AccessAdminRequest.revoke(
        requestId: 'remove', expectedRevision: '1', directoryId: 'docs')))
        .revision, '2');
    expect((await reopened.invoke(AccessAdminRequest.list(requestId: 'end')))
        .grants, isEmpty);
  });

  test('busy and repeated close preserve in-flight write and reject new work',
      () async {
    final current = client();
    final pending = current.invoke(AccessAdminRequest.set(
      requestId: 'save', expectedRevision: '0', directoryId: 'docs',
      access: DirectoryAccess.read, explicitPersonal: false,
    ));
    final busy = current.invoke(AccessAdminRequest.list(requestId: 'busy'));
    final closing = current.close();
    expect(identical(closing, current.close()), true);
    await expectLater(busy, throwsA(error('ADMIN_BUSY')));
    await expectLater(current.invoke(AccessAdminRequest.list(requestId: 'late')),
        throwsA(error('ADMIN_CLOSED')));
    expect((await pending).revision, '1');
    await closing;
    final reopened = client();
    addTearDown(reopened.close);
    expect((await reopened.invoke(AccessAdminRequest.list(requestId: 'read')))
        .grants!.single.directoryId, 'docs');
  });

  test('platform open obtains only the trusted host root', () async {
    const channel = MethodChannel('dev.wex/access_admin_host');
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    // Host contract requires a fixed leaf.
    final hostRoot = '${parent.path}/access-grants';
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'privateRoot');
      expect(call.arguments, isNull);
      return hostRoot;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
    final current = await AccessAdminClient.open(libraryPath: library);
    addTearDown(current.close);
    expect((await current.invoke(AccessAdminRequest.list(requestId: 'r1')))
        .revision, '0');
    expect(Directory(hostRoot).existsSync(), true);
    expect(Directory(root).existsSync(), false);
  });

  test('missing library and ABI mismatch remain safe and closeable', () async {
    for (final entry in {
      '/missing/private.so': 'LIBRARY_LOAD_FAILED',
      '$fixtures/admin_1.so': 'ABI_MISMATCH',
      '$fixtures/admin_2.so': 'ABI_MISMATCH',
    }.entries) {
      final current = client(path: entry.key);
      await expectLater(current.invoke(AccessAdminRequest.list(requestId: 'r1')),
          throwsA(error(entry.value)));
      await current.close();
    }
    expect(Directory(root).existsSync(), false);
  });

  test('native failures and unknown writes are never automatically retried',
      () async {
    for (final entry in {
      3: 'MALFORMED_REPLY', 4: 'MALFORMED_REPLY',
      10: 'NATIVE_INVALID_ARGUMENT', 11: 'NATIVE_CAPACITY',
      12: 'NATIVE_PANIC', 13: 'NATIVE_INTERNAL', 14: 'NATIVE_INTERNAL',
      15: 'COMMIT_UNKNOWN', 16: 'STORE_BUSY',
    }.entries) {
      final path = '$fixtures/admin_${entry.key}.so';
      final lib = DynamicLibrary.open(path);
      final count = lib.lookupFunction<Uint32 Function(), int Function()>(
          'admin_fixture_count');
      final before = count();
      final current = client(path: path);
      final operation = current.invoke(AccessAdminRequest.revoke(
          requestId: 'r1', expectedRevision: '0', directoryId: 'docs'));
      final closing = current.close();
      await expectLater(operation, throwsA(error(entry.value,
          unknown: ![10, 11, 16].contains(entry.key))));
      await closing;
      expect(count() - before, 1, reason: 'exactly one native call');
    }
  });

  test('invalid trusted root fails before worker and never reveals path', () {
    for (final value in ['', 'relative', '/private/\u0000']) {
      expect(() => AccessAdminClient.forHost(
          privateRoot: value, libraryPath: library),
          throwsA(error('HOST_UNAVAILABLE')));
    }
  });
}
