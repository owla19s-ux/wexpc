import 'dart:convert';
import 'dart:isolate';
import 'dart:typed_data';
import '../platform/access_admin_host.dart';
import '../platform/platform_library_loader.dart';
import '../platform/storage_access_service.dart';
import 'local_read_exception.dart';
import 'native_local_read.dart';

/// Trusted settings selects/registers; an authorized reader uses only read.
/// Never expose open/register, library overrides, or permission flags as AI tools.
final class LocalReadClient {
  LocalReadClient._(this._libraryPath, this._handle);
  final String _libraryPath;
  final int _handle;
  Future<Object>? _pending;
  Future<void>? _closing;
  var _closed = false;
  var _released = false;

  /// Library override is trusted Host/test composition, never user/AI input.
  static Future<LocalReadClient> open({String? libraryPath}) async {
    late String path;
    late String config;
    try {
      final layout = await AccessAdminHost.privateLayout();
      path = PlatformLibraryLoader.resolve(overridePath: libraryPath);
      config = jsonEncode({'version': 1, 'privateRoot': layout.privateRoot,
        'internalRoots': layout.internalRoots});
    } on AccessAdminHostException {
      throw const LocalReadException('HOST_UNAVAILABLE');
    } catch (_) {
      throw const LocalReadException('LIBRARY_LOAD_FAILED');
    }
    try {
      final handle = await _openWorker(path, config);
      return LocalReadClient._(path, handle);
    } on LocalReadException {
      rethrow;
    } catch (_) {
      throw const LocalReadException('WORKER_FAILED', outcomeUnknown: true);
    }
  }

  /// Explicit trusted user directory selection only; registration grants nothing.
  Future<String> register(String path) => _start(
      () => _registerWorker(_libraryPath, _handle, path), mutation: true);

  /// Permission is queried from Host on every call and is never an argument.
  Future<Uint8List> read(String directoryId, String leaf) => _start(() async {
    final status = await StorageAccessService.status();
    final granted = status.supported && status.granted && status.error == null;
    return _readWorker(_libraryPath, _handle, directoryId, leaf, granted);
  }, mutation: false);

  Future<T> _start<T extends Object>(Future<T> Function() action,
      {required bool mutation}) {
    if (_closed) return Future.error(const LocalReadException('CLIENT_CLOSED'));
    if (_pending != null) return Future.error(const LocalReadException('CLIENT_BUSY'));
    late Future<T> operation;
    operation = _translate(action, mutation).whenComplete(() {
      if (identical(_pending, operation)) _pending = null;
    });
    _pending = operation;
    return operation;
  }

  Future<T> _translate<T>(Future<T> Function() action, bool mutation) async {
    try {
      return await action();
    } on LocalReadException {
      rethrow;
    } catch (_) {
      throw LocalReadException('WORKER_FAILED', outcomeUnknown: mutation);
    }
  }

  /// Drain, never kill/timeout. Busy/failed close retains the same handle.
  /// Caller may explicitly close again; new reads remain rejected meanwhile.
  Future<void> close() {
    if (_released) return _closing ?? Future.value();
    _closed = true;
    return _closing ??= _finish();
  }

  Future<void> _finish() async {
    try {
      final pending = _pending;
      if (pending != null) {
        try {
          await pending;
        } catch (_) {
          // The original invoker owns this error; closing only drains it.
        }
      }
      try {
        await _closeWorker(_libraryPath, _handle);
      } on LocalReadException catch (error) {
        if (error.code != 'UNKNOWN_SESSION') rethrow;
        // Same library never reuses handles; already absent is safely closed.
      }
      _released = true;
    } finally {
      if (!_released) _closing = null;
    }
  }
}

// Separate top-level scopes capture only strings, int and bool. Never send
// clients/Futures, pointers, Allocators or DynamicLibrary objects across isolates.
Future<int> _openWorker(String library, String config) =>
    Isolate.run(() => NativeLocalRead.open(library).create(config),
        debugName: 'wex-local-open');
Future<String> _registerWorker(String library, int handle, String path) =>
    Isolate.run(() => NativeLocalRead.open(library).register(handle, path),
        debugName: 'wex-local-register');
Future<Uint8List> _readWorker(
    String library, int handle, String id, String leaf, bool granted) =>
    Isolate.run(() => NativeLocalRead.open(library).read(handle, id, leaf, granted),
        debugName: 'wex-local-read');
Future<void> _closeWorker(String library, int handle) =>
    Isolate.run(() => NativeLocalRead.open(library).close(handle),
        debugName: 'wex-local-close');
