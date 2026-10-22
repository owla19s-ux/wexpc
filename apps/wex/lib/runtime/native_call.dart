import 'dart:ffi';
import 'dart:typed_data';
import 'package:ffi/ffi.dart';
import 'runtime_models.dart';

typedef _CallNative = Int32 Function(Pointer<Uint8>, Uint32, Pointer<Uint8>, Uint32, Pointer<Uint32>);
typedef _CallDart = int Function(Pointer<Uint8>, int, Pointer<Uint8>, int, Pointer<Uint32>);

// Native pointers and allocations stay in the owner isolate.
final class NativeCall {
  NativeCall._(this._call, this._allocator);
  final _CallDart _call;
  final Allocator _allocator;

  factory NativeCall.open(String path, {Allocator allocator = calloc}) {
    late DynamicLibrary lib;
    try { lib = DynamicLibrary.open(path); }
    catch (_) { throw const RuntimeTransportException('LIBRARY_LOAD_FAILED'); }
    try {
      final version = lib.lookupFunction<Uint32 Function(), int Function()>('wex_runtime_abi_version');
      if (version() != 1) throw const RuntimeTransportException('ABI_MISMATCH');
      return NativeCall._(lib.lookupFunction<_CallNative,_CallDart>('wex_runtime_call'), allocator);
    } catch (_) { throw const RuntimeTransportException('ABI_MISMATCH'); }
  }

  Uint8List run(Uint8List input) {
    if (input.isEmpty || input.length > 16384) {
      throw const RuntimeTransportException('NATIVE_INVALID_ARGUMENT');
    }
    Pointer<Uint8>? request;
    Pointer<Uint8>? output;
    Pointer<Uint32>? length;
    try {
      request = _allocator.allocate<Uint8>(input.length);
      output = _allocator.allocate<Uint8>(65536);
      length = _allocator.allocate<Uint32>(sizeOf<Uint32>());
      request.asTypedList(input.length).setAll(0, input);
      length.value = 0;
      final result = _call(request, input.length, output, 65536, length);
      if (result != 0) {
        throw RuntimeTransportException(switch (result) {
          -1 => 'NATIVE_INVALID_ARGUMENT',
          -2 => 'NATIVE_CAPACITY',
          -3 => 'NATIVE_PANIC',
          _ => 'NATIVE_INTERNAL',
        });
      }
      final count = length.value;
      if (count < 1 || count > 65536) throw const RuntimeTransportException('MALFORMED_REPLY');
      return Uint8List.fromList(output.asTypedList(count));
    } on RuntimeTransportException { rethrow; }
    catch (_) { throw const RuntimeTransportException('NATIVE_INTERNAL'); }
    finally {
      if (length != null) _allocator.free(length);
      if (output != null) _allocator.free(output);
      if (request != null) _allocator.free(request);
    }
  }
}
