import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/platform/local_directory_browser.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = LocalDirectoryBrowser.channel;
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  Map<String, Object?> listing({String current = '/storage/emulated/0',
      String? parent, List<Object?>? directories}) => {
    'root': '/storage/emulated/0', 'current': current, 'parent': parent,
    'directories': directories ?? <Object?>[], 'truncated': false,
  };

  test('passes only requested path and preserves a checked child listing', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'browse');
      expect(call.arguments, {'path': null});
      return listing(directories: [
        {'name': '资料', 'path': '/storage/emulated/0/资料'},
      ]);
    });
    final value = await LocalDirectoryBrowser.browse();
    expect(value.parent, isNull);
    expect(value.directories.single.name, '资料');
    expect(value.directories.single.path, '/storage/emulated/0/资料');
  });

  test('parent of selected path is exact, and no out-of-root child accepted', () async {
    final value = LocalDirectoryListing.decode(listing(
      current: '/storage/emulated/0/A3',
      parent: '/storage/emulated/0',
      directories: [{'name':'项目','path':'/storage/emulated/0/A3/项目'}],
    ));
    expect(value.parent, '/storage/emulated/0');
    for (final child in [
      {'name':'bad', 'path':'/data/user/0/secret'},
      {'name':'..', 'path':'/storage/emulated/0/A3/..'},
    ]) {
      expect(() => LocalDirectoryListing.decode(listing(
        current: '/storage/emulated/0/A3',
        parent: '/storage/emulated/0',
        directories: [child],
      )), throwsA(isA<LocalDirectoryBrowserException>()));
    }
  });

  test('malformed and unavailable channels fail without granting anything', () async {
    messenger.setMockMethodCallHandler(channel, (_) async => {
      'root': '/storage/emulated/0', 'current': '/data/private',
      'parent': null, 'directories': [], 'truncated': false,
    });
    await expectLater(LocalDirectoryBrowser.browse(),
      throwsA(isA<LocalDirectoryBrowserException>()
        .having((e) => e.code, 'code', 'INVALID_LISTING')));
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: 'PERMISSION_REQUIRED', message: 'secret path');
    });
    await expectLater(LocalDirectoryBrowser.browse(),
      throwsA(isA<LocalDirectoryBrowserException>()
        .having((e) => e.code, 'code', 'PERMISSION_REQUIRED')));
  });
}
