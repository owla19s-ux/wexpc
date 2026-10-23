import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:wex/access_admin/access_admin_models.dart';
import 'package:wex/access_admin/grant_inventory_client.dart';
import 'package:wex/features/settings/grant_inventory_panel.dart';

class StubInventory implements GrantInventorySession {
  final entries = <AccessAdminGrant>[
    const AccessAdminGrant(
      directoryId: 'dead-session-1',
      access: DirectoryAccess.read,
      explicitPersonal: true,
    ),
    const AccessAdminGrant(
      directoryId: 'keep-session-2',
      access: DirectoryAccess.readWrite,
      explicitPersonal: false,
    ),
  ];
  int lists = 0;
  int revokes = 0;
  bool unknown = false;
  String revision = '9';
  @override
  Future<AccessAdminReply> list() async {
    lists++;
    return AccessAdminReply(revision:revision, grants:entries);
  }
  @override
  Future<AccessAdminReply> revoke(String id, String expected) async {
    revokes++;
    expect(expected, revision);
    if (unknown) {
      throw const AccessAdminException('COMMIT_UNKNOWN',
          outcomeUnknown: true);
    }
    entries.removeWhere((record) => record.directoryId == id);
    revision = '10';
    return AccessAdminReply(revision:revision);
  }
  @override
  Future<void> close() async {}
}

void main() {
  testWidgets('manual refresh, cancel, explicit revoke and verified snapshot',
      (tester) async {
    final stub = StubInventory();
    await tester.pumpWidget(MaterialApp(home: Scaffold(
      body: SingleChildScrollView(
        child: GrantInventoryPanel(openSession: () async => stub),
      ),
    )));
    expect(stub.lists, 0);
    await tester.tap(find.text('查看授权记录'));
    await tester.pumpAndSettle();
    expect(find.text('当前授权记录：2 条'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('grant-revoke-dead-session-1')));
    // A progress indicator runs while confirmation is open, so an
    // unbounded pumpAndSettle would never reach an idle animation state.
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('确认撤销授权记录'), findsOneWidget);
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    expect(stub.revokes, 0);
    await tester.tap(find.byKey(const ValueKey('grant-revoke-dead-session-1')));
    // A progress indicator runs while confirmation is open, so an
    // unbounded pumpAndSettle would never reach an idle animation state.
    await tester.pump(const Duration(milliseconds: 300));
    await tester.tap(find.text('确认撤销'));
    await tester.pumpAndSettle();
    expect(stub.revokes, 1);
    expect(stub.lists, 2);
    expect(find.text('当前授权记录：1 条'), findsOneWidget);
    expect(find.textContaining('通过重新读取核验'), findsOneWidget);
    expect(find.byKey(const ValueKey('grant-record-keep-session-2')),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('unknown commit never auto retries or claims a successful revoke',
      (tester) async {
    final stub = StubInventory()..unknown = true;
    await tester.pumpWidget(MaterialApp(home: Scaffold(
      body: SingleChildScrollView(
        child: GrantInventoryPanel(openSession: () async => stub),
      ),
    )));
    await tester.tap(find.text('查看授权记录'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('grant-revoke-dead-session-1')));
    // A progress indicator runs while confirmation is open, so an
    // unbounded pumpAndSettle would never reach an idle animation state.
    await tester.pump(const Duration(milliseconds: 300));
    await tester.tap(find.text('确认撤销'));
    await tester.pumpAndSettle();
    expect(stub.revokes, 1);
    expect(stub.lists, 1);
    expect(find.textContaining('请手动刷新'), findsOneWidget);
    expect(find.textContaining('写入结果未知'), findsOneWidget);
    expect(find.textContaining('通过重新读取核验'), findsNothing);
    await tester.tap(find.text('查看授权记录'));
    await tester.pumpAndSettle();
    expect(stub.lists, 2);
    expect(find.text('当前授权记录：2 条'), findsOneWidget);
  });
}
