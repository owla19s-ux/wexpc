import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/access_admin/access_admin_client.dart';
import 'package:wex/access_admin/access_admin_models.dart';
import 'package:wex/local_read/local_read_client.dart';
import 'package:wex/local_read/local_read_exception.dart';
import 'package:wex/local_read/native_local_read.dart';

Matcher failure(String code, {bool unknown = false}) =>
    isA<LocalReadException>().having((e) => e.code, 'code', code)
        .having((e) => e.outcomeUnknown, 'unknown', unknown);

class TrackingAllocator implements Allocator {
  TrackingAllocator({this.failAt});
  final int? failAt;
  var calls = 0;
  final live = <int>{};
  @override
  Pointer<T> allocate<T extends NativeType>(int byteCount, {int? alignment}) {
    calls++;
    if (calls == failAt) throw StateError('allocation fixture');
    final pointer = calloc.allocate<T>(byteCount, alignment: alignment);
    live.add(pointer.address);
    return pointer;
  }
  @override
  void free(Pointer<NativeType> pointer) {
    expect(live.remove(pointer.address), true);
    calloc.free(pointer);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final library = Platform.environment['WEX_TEST_LIBRARY']!;
  final fixtures = Platform.environment['WEX_TEST_FIXTURES']!;
  const host = MethodChannel('dev.wex/access_admin_host');
  const storage = MethodChannel('dev.wex/storage_access');
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  setUpAll(() async {
    await Directory(fixtures).create(recursive: true);
    for (final mode in [1, 2, 3, 4, 5, 6, 7, 8]) {
      final result = await Process.run('cc', [
        '-shared', '-fPIC', '-Wall', '-Wextra', '-Werror', '-DMODE=$mode',
        '../../tools/native-fixtures/local_read_failure.c',
        '-o', '$fixtures/read_$mode.so',
      ]);
      expect(result.exitCode, 0, reason: result.stderr.toString());
    }
  });
  late Directory base;
  late String private;
  late String public;
  var granted = true;
  var statusCalls = 0;
  setUp(() async {
    base = await Directory.systemTemp.createTemp('wex-read-client-');
    private = '${base.path}/private-内部';
    public = '${base.path}/public-资料';
    await Directory('$private/files').create(recursive: true);
    await Directory(public).create();
    await File('$public/file').writeAsBytes([0, 255, 42, 0]);
    granted = true;
    statusCalls = 0;
    messenger.setMockMethodCallHandler(host, (call) async {
      expect(call.method, 'privateLayout');
      expect(call.arguments, isNull);
      return {'version': 1, 'privateRoot': '$private/files/access-grants',
        'internalRoots': [private]};
    });
    messenger.setMockMethodCallHandler(storage, (call) async {
      expect(call.method, 'status');
      expect(call.arguments, isNull);
      statusCalls++;
      return {'supported': true, 'granted': granted, 'error': null};
    });
  });
  tearDown(() async {
    messenger.setMockMethodCallHandler(host, null);
    messenger.setMockMethodCallHandler(storage, null);
    await base.delete(recursive: true);
  });
  Future<LocalReadClient> client({String? path}) =>
      LocalReadClient.open(libraryPath: path ?? library);
  Future<void> grant(String id, {bool personal = true}) async {
    final admin = AccessAdminClient.forHost(
        privateRoot: '$private/files/access-grants', libraryPath: library);
    try {
      final state = await admin.invoke(AccessAdminRequest.list(requestId: 'load'));
      await admin.invoke(AccessAdminRequest.set(
          requestId: 'save', expectedRevision: state.revision, directoryId: id,
          access: DirectoryAccess.read, explicitPersonal: personal));
    } finally {
      await admin.close();
    }
  }

  test('Host layout reaches actual Rust session across background isolates', () async {
    final current = await client();
    addTearDown(current.close);
    final id = await current.register(public);
    expect(id, matches(RegExp(r'^[0-9a-f]{64}$')));
    await expectLater(current.read(id, 'file'), throwsA(failure('NO_GRANT')));
    await grant(id, personal: false);
    await expectLater(current.read(id, 'file'), throwsA(failure('PERSONAL_REQUIRED')));
    await grant(id);
    expect(await current.read(id, 'file'), Uint8List.fromList([0, 255, 42, 0]));
    expect(statusCalls, 3);
  });

  test('every read refreshes system status and unavailable status denies', () async {
    final current = await client();
    addTearDown(current.close);
    final id = await current.register(public);
    await grant(id);
    expect(await current.read(id, 'file'), [0, 255, 42, 0]);
    granted = false;
    await expectLater(current.read(id, 'file'), throwsA(failure('SYSTEM_UNAVAILABLE')));
    messenger.setMockMethodCallHandler(storage, null);
    await expectLater(current.read(id, 'file'), throwsA(failure('SYSTEM_UNAVAILABLE')));
    granted = true;
    messenger.setMockMethodCallHandler(storage,
        (_) async => {'supported': true, 'granted': true, 'error': null});
    expect(await current.read(id, 'file'), [0, 255, 42, 0]);
  });

  test('revocation affects the next read without rebuilding the client', () async {
    final current = await client();
    addTearDown(current.close);
    final id = await current.register(public);
    await grant(id);
    expect(await current.read(id, 'file'), [0, 255, 42, 0]);
    final admin = AccessAdminClient.forHost(
        privateRoot: '$private/files/access-grants', libraryPath: library);
    final state = await admin.invoke(AccessAdminRequest.list(requestId: 'load'));
    await admin.invoke(AccessAdminRequest.revoke(
        requestId: 'remove', expectedRevision: state.revision, directoryId: id));
    await admin.close();
    await expectLater(current.read(id, 'file'), throwsA(failure('NO_GRANT')));
  });

  test('single flight and close drain preserve the in-flight registration', () async {
    final current = await client();
    final pending = current.register(public);
    final busy = current.register(public);
    final closing = current.close();
    expect(identical(closing, current.close()), true);
    await expectLater(busy, throwsA(failure('CLIENT_BUSY')));
    await expectLater(current.register(public), throwsA(failure('CLIENT_CLOSED')));
    expect(await pending, hasLength(64));
    await closing;
    await current.close();
    await expectLater(current.read('a' * 64, 'file'), throwsA(failure('CLIENT_CLOSED')));
  });

  test('private registrations reject and new sessions do not restore old IDs', () async {
    final first = await client();
    await expectLater(first.register(private), throwsA(failure('REGISTER_FAILED')));
    final id = await first.register(public);
    await grant(id);
    await first.close();
    final next = await client();
    addTearDown(next.close);
    final nextId = await next.register(public);
    expect(nextId, isNot(id));
    await expectLater(next.read(id, 'file'), throwsA(failure('UNKNOWN_DIRECTORY')));
    await expectLater(next.read(nextId, 'file'), throwsA(failure('NO_GRANT')));
  });

  test('binary empty exact oversized and invalid direct names remain bounded', () async {
    final current = await client();
    addTearDown(current.close);
    final id = await current.register(public);
    await grant(id);
    await File('$public/empty').writeAsBytes([]);
    await File('$public/exact').writeAsBytes(List.filled(65536, 7));
    await File('$public/large').writeAsBytes(List.filled(65537, 7));
    expect(await current.read(id, 'empty'), isEmpty);
    expect((await current.read(id, 'exact')).length, 65536);
    await expectLater(current.read(id, 'large'), throwsA(failure('TOO_LARGE')));
    await expectLater(current.read(id, '../file'),
        throwsA(failure('NATIVE_INVALID_ARGUMENT')));
  });

  test('missing Host fails without opening a native session or legacy fallback', () async {
    messenger.setMockMethodCallHandler(host, null);
    await expectLater(client(), throwsA(failure('HOST_UNAVAILABLE')));
    expect(Directory('$private/files/access-grants').existsSync(), false);
  });

  test('missing library and mismatched ABI fail safely', () async {
    await expectLater(client(path: '/missing/read.so'),
        throwsA(failure('LIBRARY_LOAD_FAILED')));
    await expectLater(client(path: '$fixtures/read_1.so'),
        throwsA(failure('ABI_MISMATCH')));
  });

  test('malformed native handles and panic opens are not automatically retried', () async {
    for (final entry in {2: 'NATIVE_PANIC', 3: 'MALFORMED_REPLY', 4: 'MALFORMED_REPLY'}.entries) {
      final path = '$fixtures/read_${entry.key}.so';
      final lib = DynamicLibrary.open(path);
      final count = lib.lookupFunction<Uint32 Function(), int Function()>('read_fixture_opens');
      final before = count();
      await expectLater(client(path: path),
          throwsA(failure(entry.value, unknown: true)));
      expect(count() - before, 1);
    }
  });

  test('malformed IDs and oversized or inconsistent read lengths never escape', () async {
    for (final mode in [5, 6, 7]) {
      final current = await client(path: '$fixtures/read_$mode.so');
      addTearDown(current.close);
      if (mode == 5) {
        await expectLater(current.register(public),
            throwsA(failure('MALFORMED_REPLY', unknown: true)));
      } else {
        await expectLater(current.read('a' * 64, 'file'),
            throwsA(failure('MALFORMED_REPLY')));
      }
    }
  });

  test('Busy close retains its native handle for a later explicit close', () async {
    final current = await client(path: '$fixtures/read_8.so');
    final lib = DynamicLibrary.open('$fixtures/read_8.so');
    final calls = lib.lookupFunction<Uint32 Function(), int Function()>('read_fixture_closes');
    final before = calls();
    await expectLater(current.close(), throwsA(failure('NATIVE_BUSY')));
    await expectLater(current.register(public), throwsA(failure('CLIENT_CLOSED')));
    await current.close();
    expect(calls() - before, 2);
  });

  test('native arena releases every allocation on each partial allocation failure', () {
    for (final failAt in [1, 2]) {
      final memory = TrackingAllocator(failAt: failAt);
      final native = NativeLocalRead.open(library, allocator: memory);
      expect(() => native.create('{}'), throwsA(failure('NATIVE_INTERNAL')));
      expect(memory.live, isEmpty);
    }
    final memory = TrackingAllocator();
    final native = NativeLocalRead.open(library, allocator: memory);
    expect(() => native.create('{}'), throwsA(failure('INVALID_LAYOUT')));
    expect(memory.live, isEmpty);
    // Register/read allocate all buffers before calling the unknown handle.
    for (final failAt in [1, 2, 3, 4]) {
      final allocation = TrackingAllocator(failAt: failAt);
      final api = NativeLocalRead.open(library, allocator: allocation);
      expect(() => api.register(999999, public),
          throwsA(isA<LocalReadException>()));
      expect(allocation.live, isEmpty);
    }
    for (final failAt in [1, 2, 3, 4]) {
      final allocation = TrackingAllocator(failAt: failAt);
      final api = NativeLocalRead.open(library, allocator: allocation);
      expect(() => api.read(999999, 'a' * 64, 'file', true),
          throwsA(isA<LocalReadException>()));
      expect(allocation.live, isEmpty);
    }
  });
}
