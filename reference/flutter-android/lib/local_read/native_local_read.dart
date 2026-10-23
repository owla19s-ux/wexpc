import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';
import 'package:ffi/ffi.dart';
import 'local_read_exception.dart';

typedef _OpenN = Int32 Function(Pointer<Uint8>, Uint32, Pointer<Uint64>);
typedef _OpenD = int Function(Pointer<Uint8>, int, Pointer<Uint64>);
typedef _RegisterN = Int32 Function(Uint64, Pointer<Uint8>, Uint32,
    Pointer<Uint8>, Uint32, Pointer<Uint32>);
typedef _RegisterD = int Function(int, Pointer<Uint8>, int,
    Pointer<Uint8>, int, Pointer<Uint32>);
typedef _ReadN = Int32 Function(Uint64, Pointer<Uint8>, Uint32,
    Pointer<Uint8>, Uint32, Uint8, Pointer<Uint8>, Uint32, Pointer<Uint32>);
typedef _ReadD = int Function(int, Pointer<Uint8>, int,
    Pointer<Uint8>, int, int, Pointer<Uint8>, int, Pointer<Uint32>);
typedef _CloseN = Int32 Function(Uint64);
typedef _CloseD = int Function(int);

/// Trusted low-level binding; all pointer/arena state stays in this isolate.
final class NativeLocalRead {
  NativeLocalRead._(this._open, this._register, this._read, this._close, this._allocator);
  final _OpenD _open;
  final _RegisterD _register;
  final _ReadD _read;
  final _CloseD _close;
  final Allocator _allocator;
  static final _id = RegExp(r'^[0-9a-f]{64}$');
  static const _codes = {
    -1: 'NATIVE_INVALID_ARGUMENT', -2: 'NATIVE_CAPACITY', -3: 'NATIVE_PANIC',
    -4: 'INVALID_LAYOUT', -5: 'NATIVE_UNAVAILABLE', -6: 'NATIVE_BUSY',
    -7: 'SESSION_LIMIT', -8: 'UNKNOWN_SESSION', -9: 'REGISTER_FAILED',
    -10: 'NO_GRANT', -11: 'PERSONAL_REQUIRED', -12: 'SYSTEM_UNAVAILABLE',
    -13: 'OPERATION_DENIED', -14: 'READ_FAILED', -17: 'UNKNOWN_DIRECTORY',
    -18: 'TOO_LARGE', -19: 'CHANGED',
  };

  factory NativeLocalRead.open(String path, {Allocator allocator = calloc}) {
    late DynamicLibrary library;
    try {
      library = DynamicLibrary.open(path);
    } catch (_) {
      throw const LocalReadException('LIBRARY_LOAD_FAILED');
    }
    try {
      final version = library.lookupFunction<Uint32 Function(), int Function()>(
          'wex_local_read_abi_version');
      if (version() != 1) throw const LocalReadException('ABI_MISMATCH');
      return NativeLocalRead._(
          library.lookupFunction<_OpenN, _OpenD>('wex_local_read_open'),
          library.lookupFunction<_RegisterN, _RegisterD>('wex_local_read_register'),
          library.lookupFunction<_ReadN, _ReadD>('wex_local_read_bytes'),
          library.lookupFunction<_CloseN, _CloseD>('wex_local_read_close'), allocator);
    } catch (_) {
      throw const LocalReadException('ABI_MISMATCH');
    }
  }

  static void _status(int code, Set<int> allowed, {required bool mutation}) {
    if (code == 0) return;
    final name = _codes[code];
    if (name == null || !allowed.contains(code)) {
      throw LocalReadException('MALFORMED_REPLY', outcomeUnknown: mutation);
    }
    throw LocalReadException(name,
        outcomeUnknown: mutation && (code == -3 || code == -5));
  }

  static void _handle(int handle) {
    if (handle <= 0 || handle > 0x7fffffffffffffff) {
      throw const LocalReadException('NATIVE_INVALID_ARGUMENT');
    }
  }

  static Pointer<Uint8> _input(Arena arena, List<int> bytes) {
    final buffer = arena<Uint8>(bytes.length);
    buffer.asTypedList(bytes.length).setAll(0, bytes);
    return buffer;
  }

  int create(String config) {
    final bytes = utf8.encode(config);
    if (bytes.isEmpty || bytes.length > 65536) {
      throw const LocalReadException('NATIVE_INVALID_ARGUMENT');
    }
    var entered = false;
    try {
      return using<int>((arena) {
        final input = _input(arena, bytes);
        final handle = arena<Uint64>();
        handle.value = 0;
        entered = true;
        final code = _open(input, bytes.length, handle);
        if (code != 0 && handle.value != 0) {
          throw const LocalReadException('MALFORMED_REPLY', outcomeUnknown: true);
        }
        _status(code, {-1, -3, -4, -5, -6, -7}, mutation: true);
        if (handle.value <= 0 || handle.value > 0x7fffffffffffffff) {
          throw const LocalReadException('MALFORMED_REPLY', outcomeUnknown: true);
        }
        return handle.value;
      }, _allocator);
    } on LocalReadException {
      rethrow;
    } catch (_) {
      throw LocalReadException('NATIVE_INTERNAL', outcomeUnknown: entered);
    }
  }

  String register(int handle, String path) {
    _handle(handle);
    final bytes = utf8.encode(path);
    if (!path.startsWith('/') || path.contains('\u0000') ||
        bytes.isEmpty || bytes.length > 4096) {
      throw const LocalReadException('NATIVE_INVALID_ARGUMENT');
    }
    var entered = false;
    try {
      return using<String>((arena) {
        final input = _input(arena, bytes);
        final output = arena<Uint8>(64);
        final length = arena<Uint32>();
        length.value = 0;
        entered = true;
        final code = _register(handle, input, bytes.length, output, 64, length);
        if (code != 0 && length.value != 0) {
          throw const LocalReadException('MALFORMED_REPLY', outcomeUnknown: true);
        }
        _status(code, {-1, -2, -3, -5, -6, -8, -9}, mutation: true);
        if (length.value != 64) {
          throw const LocalReadException('MALFORMED_REPLY', outcomeUnknown: true);
        }
        final id = String.fromCharCodes(output.asTypedList(64));
        if (!_id.hasMatch(id)) {
          throw const LocalReadException('MALFORMED_REPLY', outcomeUnknown: true);
        }
        return id;
      }, _allocator);
    } on LocalReadException {
      rethrow;
    } catch (_) {
      throw LocalReadException('NATIVE_INTERNAL', outcomeUnknown: entered);
    }
  }

  Uint8List read(int handle, String id, String leaf, bool systemAccess) {
    _handle(handle);
    final leafBytes = utf8.encode(leaf);
    if (!_id.hasMatch(id) || leafBytes.isEmpty || leafBytes.length > 4096 ||
        leaf.contains('/') || leaf.contains('\u0000') || leaf == '.' || leaf == '..') {
      throw const LocalReadException('NATIVE_INVALID_ARGUMENT');
    }
    try {
      return using<Uint8List>((arena) {
        final directory = _input(arena, utf8.encode(id));
        final name = _input(arena, leafBytes);
        final output = arena<Uint8>(65536);
        final length = arena<Uint32>();
        length.value = 0;
        final code = _read(handle, directory, 64, name, leafBytes.length,
            systemAccess ? 1 : 0, output, 65536, length);
        if (code != 0 && length.value != 0) {
          throw const LocalReadException('MALFORMED_REPLY');
        }
        _status(code, {-1, -2, -3, -5, -6, -8, -10, -11, -12, -13, -14, -17, -18, -19},
            mutation: false);
        if (length.value > 65536) {
          throw const LocalReadException('MALFORMED_REPLY');
        }
        return Uint8List.fromList(output.asTypedList(length.value));
      }, _allocator);
    } on LocalReadException {
      rethrow;
    } catch (_) {
      throw const LocalReadException('NATIVE_INTERNAL');
    }
  }

  void close(int handle) {
    _handle(handle);
    try {
      _status(_close(handle), {-3, -5, -6, -8}, mutation: true);
    } on LocalReadException {
      rethrow;
    } catch (_) {
      throw const LocalReadException('NATIVE_INTERNAL', outcomeUnknown: true);
    }
  }
}
