import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'access_admin_models.dart';

typedef _AdminNative = Int32 Function(
    Pointer<Uint8>, Uint32, Pointer<Uint8>, Uint32,
    Pointer<Uint8>, Uint32, Pointer<Uint32>);
typedef _AdminDart = int Function(
    Pointer<Uint8>, int, Pointer<Uint8>, int,
    Pointer<Uint8>, int, Pointer<Uint32>);

/// All handles, pointers and allocations remain in the calling isolate.
final class NativeAccessAdmin {
  NativeAccessAdmin._(this._call, this._allocator);
  final _AdminDart _call;
  final Allocator _allocator;

  factory NativeAccessAdmin.open(String libraryPath,
      {Allocator allocator = calloc}) {
    late DynamicLibrary library;
    try {
      library = DynamicLibrary.open(libraryPath);
    } catch (_) {
      throw const AccessAdminException('LIBRARY_LOAD_FAILED');
    }
    try {
      final version = library.lookupFunction<
          Uint32 Function(), int Function()>('wex_access_admin_abi_version');
      if (version() != 1) {
        throw const AccessAdminException('ABI_MISMATCH');
      }
      return NativeAccessAdmin._(
          library.lookupFunction<_AdminNative, _AdminDart>(
              'wex_access_admin_call'), allocator);
    } catch (_) {
      throw const AccessAdminException('ABI_MISMATCH');
    }
  }

  Uint8List run(String privateRoot, Uint8List input,
      {required bool mutation}) {
    final rootBytes = utf8.encode(privateRoot);
    if (!privateRoot.startsWith('/') || privateRoot.contains('\u0000') ||
        rootBytes.isEmpty || rootBytes.length > 4096 ||
        input.isEmpty || input.length > 16384) {
      throw const AccessAdminException('NATIVE_INVALID_ARGUMENT');
    }
    Pointer<Uint8>? root;
    Pointer<Uint8>? request;
    Pointer<Uint8>? output;
    Pointer<Uint32>? length;
    var entered = false;
    try {
      root = _allocator.allocate<Uint8>(rootBytes.length);
      request = _allocator.allocate<Uint8>(input.length);
      output = _allocator.allocate<Uint8>(65536);
      length = _allocator.allocate<Uint32>(sizeOf<Uint32>());
      root.asTypedList(rootBytes.length).setAll(0, rootBytes);
      request.asTypedList(input.length).setAll(0, input);
      length.value = 0;
      entered = true;
      final result = _call(root, rootBytes.length, request, input.length,
          output, 65536, length);
      if (result != 0) {
        throw AccessAdminException(switch (result) {
          -1 => 'NATIVE_INVALID_ARGUMENT',
          -2 => 'NATIVE_CAPACITY',
          -3 => 'NATIVE_PANIC',
          _ => 'NATIVE_INTERNAL',
        }, outcomeUnknown: mutation && result != -1 && result != -2);
      }
      final count = length.value;
      if (count < 1 || count > 65536) {
        throw AccessAdminException('MALFORMED_REPLY',
            outcomeUnknown: mutation);
      }
      return Uint8List.fromList(output.asTypedList(count));
    } on AccessAdminException {
      rethrow;
    } catch (_) {
      throw AccessAdminException('NATIVE_INTERNAL',
          outcomeUnknown: mutation && entered);
    } finally {
      if (length != null) _allocator.free(length);
      if (output != null) _allocator.free(output);
      if (request != null) _allocator.free(request);
      if (root != null) _allocator.free(root);
    }
  }
}
