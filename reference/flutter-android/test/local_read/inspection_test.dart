import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/access_admin/access_admin_client.dart';
import 'package:wex/access_admin/access_admin_models.dart';
import 'package:wex/local_read/local_read_client.dart';
import 'package:wex/local_read/local_read_inspector.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final library = Platform.environment['WEX_TEST_LIBRARY']!;
  const host = MethodChannel('dev.wex/access_admin_host');
  const storage = MethodChannel('dev.wex/storage_access');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  late Directory root;
  late String private;
  late String resource;
  var granted = true;

  setUp(() async {
    root = await Directory.systemTemp.createTemp('wex-inspect-');
    private = '${root.path}/private';
    resource = '${root.path}/resource';
    await Directory('$private/files').create(recursive: true);
    await Directory(resource).create();
    await File('$resource/demo.txt').writeAsString('hello world');
    granted = true;
    messenger.setMockMethodCallHandler(host, (call) async {
      if (call.method == 'privateRoot') return '$private/files/access-grants';
      if (call.method == 'privateLayout') {
        return {'version':1,'privateRoot':'$private/files/access-grants',
          'internalRoots':[private]};
      }
      fail('unexpected Host method');
    });
    messenger.setMockMethodCallHandler(storage, (_) async =>
      {'supported':true,'granted':granted});
  });

  tearDown(() async {
    messenger.setMockMethodCallHandler(host, null);
    messenger.setMockMethodCallHandler(storage, null);
    await root.delete(recursive: true);
  });

  Future<LocalReadInspector> inspector() async {
    final reader = await LocalReadClient.open(libraryPath: library);
    final admin = AccessAdminClient.forHost(
        privateRoot:'$private/files/access-grants', libraryPath:library);
    return LocalReadInspector(reader, admin);
  }

  test('explicit read is real and grant is revoked before content returned', () async {
    final current = await inspector();
    addTearDown(current.close);
    await current.selectDirectory(resource);
    expect(await current.readOnce('demo.txt'), 'hello world'.codeUnits);
    final admin = AccessAdminClient.forHost(
        privateRoot:'$private/files/access-grants', libraryPath:library);
    addTearDown(admin.close);
    expect((await admin.invoke(AccessAdminRequest.list(requestId:'final')))
        .grants, isEmpty);
    expect(current.blocked, false);
  });

  test('invalid leaf is rejected before any grant is written', () async {
    final current = await inspector();
    addTearDown(current.close);
    await current.selectDirectory(resource);
    await expectLater(current.readOnce('../demo.txt'),
        throwsA(isA<LocalInspectionException>()
            .having((e) => e.code, 'code', 'INVALID_FILE')));
    final admin = AccessAdminClient.forHost(
        privateRoot:'$private/files/access-grants', libraryPath:library);
    addTearDown(admin.close);
    expect((await admin.invoke(AccessAdminRequest.list(requestId:'final')))
        .grants, isEmpty);
  });

  test('system revocation denies read and temporary grant is still revoked', () async {
    final current = await inspector();
    addTearDown(current.close);
    await current.selectDirectory(resource);
    granted = false;
    await expectLater(current.readOnce('demo.txt'),
        throwsA(isA<Exception>()));
    final admin = AccessAdminClient.forHost(
        privateRoot:'$private/files/access-grants', libraryPath:library);
    addTearDown(admin.close);
    expect((await admin.invoke(AccessAdminRequest.list(requestId:'final')))
        .grants, isEmpty);
  });

  test('private roots refuse registration without creating a grant', () async {
    final current = await inspector();
    addTearDown(current.close);
    await expectLater(current.selectDirectory(private),
        throwsA(isA<Exception>()));
    expect(current.hasDirectory, false);
  });
}
