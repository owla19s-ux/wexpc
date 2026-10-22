import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/access_admin/access_admin_models.dart';
import 'package:wex/access_admin/access_admin_codec.dart';
import 'package:wex/access_admin/native_access_admin.dart';

class TrackingAllocator implements Allocator {
  TrackingAllocator({this.failAt});
  final int? failAt;
  var count = 0;
  final active = <int>{};
  @override
  Pointer<T> allocate<T extends NativeType>(int byteCount, {int? alignment}) {
    if (++count == failAt) throw StateError('private allocation detail');
    final value = calloc.allocate<T>(byteCount, alignment: alignment);
    active.add(value.address);
    return value;
  }
  @override
  void free(Pointer pointer) {
    expect(active.remove(pointer.address), true);
    calloc.free(pointer);
  }
}

void main() {
  final library = Platform.environment['WEX_TEST_LIBRARY']!;
  final fixtures = Platform.environment['WEX_TEST_FIXTURES']!;
  final request = AccessAdminCodec.encode(
      AccessAdminRequest.list(requestId: 'r1'));

  test('all partial allocation failures release prior buffers before call', () {
    for (final failAt in [1, 2, 3, 4]) {
      final allocator = TrackingAllocator(failAt: failAt);
      final native = NativeAccessAdmin.open(library, allocator: allocator);
      expect(() => native.run('/not-created', request, mutation: true),
          throwsA(isA<AccessAdminException>()
            .having((e) => e.code, 'code', 'NATIVE_INTERNAL')
            .having((e) => e.outcomeUnknown, 'unknown', false)));
      expect(allocator.active, isEmpty);
    }
  });

  test('native error and output length failure release all four buffers', () {
    for (final mode in [3, 4, 10, 11, 12, 13, 14]) {
      final allocator = TrackingAllocator();
      final native = NativeAccessAdmin.open(
          '$fixtures/admin_$mode.so', allocator: allocator);
      try {
        native.run('/private', request, mutation: true);
      } on AccessAdminException {
        // Mode4 returns bytes for the codec to reject, others fail in native.
      }
      expect(allocator.count, 4);
      expect(allocator.active, isEmpty);
    }
  });

  test('real successful buffers are copied and freed before decoding', () async {
    final parent = await Directory.systemTemp.createTemp('wex-admin-memory-');
    try {
      final allocator = TrackingAllocator();
      final native = NativeAccessAdmin.open(library, allocator: allocator);
      for (var i = 0; i < 8; i++) {
        final result = native.run('${parent.path}/private', request,
            mutation: false);
        expect(allocator.active, isEmpty);
        expect(AccessAdminCodec.decode(result,
            AccessAdminRequest.list(requestId: 'r1')).revision, '0');
      }
      expect(allocator.count, 32);
    } finally { await parent.delete(recursive: true); }
  });

  test('invalid buffer and root bounds do not allocate or enter native', () {
    final allocator = TrackingAllocator();
    final native = NativeAccessAdmin.open(library, allocator: allocator);
    for (final input in [Uint8List(0), Uint8List(16385)]) {
      expect(() => native.run('/private', input, mutation: true),
          throwsA(isA<AccessAdminException>()
            .having((e) => e.outcomeUnknown, 'unknown', false)));
    }
    expect(allocator.count, 0);
  });
}
