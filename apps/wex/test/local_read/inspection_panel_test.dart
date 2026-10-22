import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/features/settings/local_read_inspection_panel.dart';
import 'package:wex/local_read/local_read_inspector.dart';
import 'package:wex/platform/local_directory_browser.dart';

class FakeInspectionSession implements LocalInspectionSession {
  @override
  bool blocked = false;
  String? selected;
  int reads = 0;
  int revocations = 0;

  @override
  Future<void> selectDirectory(String path) async { selected = path; }

  @override
  Future<Uint8List> readOnce(String leaf) async {
    reads++;
    if (leaf != 'file.txt') throw const LocalInspectionException('INVALID_FILE');
    return Uint8List.fromList('verified data'.codeUnits);
  }

  @override
  Future<void> reviewAndRevoke() async { revocations++; blocked = false; }

  @override
  Future<void> close() async {}
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = LocalDirectoryBrowser.channel;
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  testWidgets('select, cancel, confirm and show result only after success',
      (tester) async {
    final session = FakeInspectionSession();
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.arguments, {'path': null});
      return {'root':'/storage/emulated/0', 'current':'/storage/emulated/0',
        'parent':null, 'directories':[], 'truncated':false};
    });
    await tester.pumpWidget(MaterialApp(home: Scaffold(
      body: SingleChildScrollView(
        child: LocalReadInspectionPanel(openInspector: () async => session),
      ),
    )));
    expect(find.textContaining('Space 关联不会授权'), findsOneWidget);
    await tester.tap(find.text('选择目录'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('选择此目录'));
    await tester.pumpAndSettle();
    expect(session.selected, '/storage/emulated/0');
    expect(find.textContaining('尚未授权读取'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('local-read-leaf')), 'file.txt');
    await tester.ensureVisible(find.byKey(const ValueKey('local-read-submit')));
    await tester.tap(find.byKey(const ValueKey('local-read-submit')));
    await tester.pumpAndSettle();
    expect(find.text('确认本次只读授权'), findsOneWidget);
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    expect(session.reads, 0);
    expect(find.byKey(const ValueKey('local-read-preview')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('local-read-submit')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('授权并读取'));
    await tester.pumpAndSettle();
    expect(session.reads, 1);
    expect(find.textContaining('临时授权已撤销'), findsOneWidget);
    expect(find.textContaining('verified data'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('picker unavailable does not create a session', (tester) async {
    var opened = 0;
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: 'PERMISSION_REQUIRED');
    });
    await tester.pumpWidget(MaterialApp(home: Scaffold(
      body: LocalReadInspectionPanel(openInspector: () async {
        opened++;
        return FakeInspectionSession();
      }),
    )));
    await tester.tap(find.text('选择目录'));
    await tester.pumpAndSettle();
    expect(find.textContaining('PERMISSION_REQUIRED'), findsOneWidget);
    expect(opened, 0);
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
  });
}
