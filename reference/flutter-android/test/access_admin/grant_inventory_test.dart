import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:wex/access_admin/access_admin_client.dart';
import 'package:wex/access_admin/access_admin_models.dart';
import 'package:wex/access_admin/grant_inventory_client.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final library = Platform.environment['WEX_TEST_LIBRARY']!;
  late Directory temp;
  late String root;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('wex-grant-inventory-');
    root = '${temp.path}/private-grants';
  });
  tearDown(() async { await temp.delete(recursive: true); });

  AccessAdminClient admin() =>
      AccessAdminClient.forHost(privateRoot: root, libraryPath: library);
  GrantInventoryClient inventory() => GrantInventoryClient.forAdmin(admin());

  test('real Rust grant inventory lists, revokes only selected ID and verifies', () async {
    final seed = admin();
    final empty = await seed.invoke(AccessAdminRequest.list(requestId: 'initial'));
    expect(empty.grants, isEmpty);
    final one = await seed.invoke(AccessAdminRequest.set(
      requestId: 'add-a', expectedRevision: empty.revision,
      directoryId: 'stale-session-a', access: DirectoryAccess.read,
      explicitPersonal: true));
    await seed.invoke(AccessAdminRequest.set(
      requestId: 'add-b', expectedRevision: one.revision,
      directoryId: 'retained-b', access: DirectoryAccess.readWrite,
      explicitPersonal: false));
    await seed.close();

    final current = inventory();
    addTearDown(current.close);
    final snapshot = await current.list();
    expect(snapshot.grants!.length, 2);
    expect(snapshot.grants!.map((grant) => grant.directoryId).toSet(),
        {'stale-session-a', 'retained-b'});
    final response = await current.revoke('stale-session-a', snapshot.revision);
    expect(response.revision, '3');
    final refreshed = await current.list();
    expect(refreshed.grants!.map((g) => g.directoryId), ['retained-b']);
    expect(refreshed.grants!.single.access, DirectoryAccess.readWrite);
  });

  test('stale CAS revision fails, is not retried, and preserves current grants', () async {
    final current = inventory();
    addTearDown(current.close);
    final before = await current.list();
    expect(before.revision, '0');
    final other = admin();
    await other.invoke(AccessAdminRequest.set(
      requestId:'external', expectedRevision:before.revision,
      directoryId:'concurrent-id', access:DirectoryAccess.write,
      explicitPersonal:false));
    await other.close();
    await expectLater(current.revoke('concurrent-id', before.revision),
      throwsA(isA<AccessAdminException>().having(
        (e) => e.code, 'code', 'REVISION_CONFLICT')));
    final after = await current.list();
    expect(after.grants!.single.directoryId, 'concurrent-id');
    expect(after.revision, '1');
  });

  test('opaque invalid IDs do not bypass Rust request validation', () async {
    final current = inventory();
    addTearDown(current.close);
    expect(() => current.revoke('../private', '0'),
      throwsA(isA<AccessAdminException>().having(
        (e) => e.code, 'code', 'INVALID_REQUEST')));
    expect((await current.list()).grants, isEmpty);
  });
}
