import 'dart:ffi';
import 'dart:io';
import 'package:ffi/ffi.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/runtime/native_call.dart';
import 'package:wex/runtime/runtime_models.dart';

class TrackedAllocator implements Allocator {
  TrackedAllocator({this.failAt});
  final int? failAt;
  int count = 0;
  final active = <int>{};
  @override
  Pointer<T> allocate<T extends NativeType>(int byteCount, {int? alignment}) {
    count++;
    if (count == failAt) throw StateError('allocation failure');
    final p = calloc.allocate<T>(byteCount, alignment:alignment);
    active.add(p.address);
    return p;
  }
  @override
  void free(Pointer pointer) {
    expect(active.remove(pointer.address), true, reason:'free exactly once');
    calloc.free(pointer);
  }
}
Matcher transport(String code) => isA<RuntimeTransportException>().having((e)=>e.code,'code',code);
void main() {
  final root = Platform.environment['WEX_TEST_FIXTURES']!;
  final real = Platform.environment['WEX_TEST_LIBRARY']!;
  final request = RuntimeCodec.encode(RuntimeRequest.ping(requestId:'r1'));
  test('wrong ABI and missing symbol reject before executing calls', () {
    for (final mode in [1,2]) {
      expect(()=>NativeCall.open('$root/fixture_$mode.so'), throwsA(transport('ABI_MISMATCH')));
    }
  });
  test('native error codes and invalid output length free every allocation', () {
    final modes = {3:'MALFORMED_REPLY',10:'NATIVE_INVALID_ARGUMENT',11:'NATIVE_CAPACITY',
      12:'NATIVE_PANIC',13:'NATIVE_INTERNAL',14:'NATIVE_INTERNAL'};
    for (final mode in modes.entries) {
      final allocator = TrackedAllocator();
      final native = NativeCall.open('$root/fixture_${mode.key}.so',allocator:allocator);
      expect(()=>native.run(request), throwsA(transport(mode.value)));
      expect(allocator.active,isEmpty);
      expect(allocator.count,3);
    }
  });
  test('partial allocation failure frees earlier buffers', () {
    for(final failAt in [1,2,3]) {
      final allocator = TrackedAllocator(failAt:failAt);
      final native = NativeCall.open(real,allocator:allocator);
      expect(()=>native.run(request), throwsA(transport('NATIVE_INTERNAL')));
      expect(allocator.active,isEmpty);
    }
  });
  test('real successful native calls release buffers before returning bytes', () {
    final allocator = TrackedAllocator();
    final native = NativeCall.open(real,allocator:allocator);
    for(var i=0;i<50;i++) {
      final result = native.run(request);
      expect(allocator.active,isEmpty);
      expect(RuntimeCodec.decode(result,RuntimeRequest.ping(requestId:'r1')).value,'pong');
    }
    expect(allocator.count,150);
  });
}
