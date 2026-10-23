import 'dart:convert';
import 'dart:isolate';

import '../platform/access_admin_host.dart';
import '../platform/platform_library_loader.dart';
import 'access_admin_codec.dart';
import 'access_admin_models.dart';
import 'native_access_admin.dart';

/// Trusted settings composition only. Never register invoke as an AI tool.
final class AccessAdminClient {
  AccessAdminClient._(this._root, this._libraryPath);

  /// The library override is only for trusted hosts/tests, never user input.
  static Future<AccessAdminClient> open({String? libraryPath}) async {
    try {
      return AccessAdminClient.forHost(
          privateRoot: await AccessAdminHost.privateRoot(),
          libraryPath: PlatformLibraryLoader.resolve(
              overridePath: libraryPath));
    } on AccessAdminHostException {
      throw const AccessAdminException('HOST_UNAVAILABLE');
    } on AccessAdminException {
      rethrow;
    } catch (_) {
      throw const AccessAdminException('LIBRARY_LOAD_FAILED');
    }
  }

  /// Trusted composition seam for platform adapters and host-native tests.
  /// This constructor does not make an arbitrary path private or authorized.
  factory AccessAdminClient.forHost({
    required String privateRoot,
    required String libraryPath,
  }) {
    if (!privateRoot.startsWith('/') || privateRoot.contains('\u0000') ||
        utf8.encode(privateRoot).length > 4096) {
      throw const AccessAdminException('HOST_UNAVAILABLE');
    }
    return AccessAdminClient._(privateRoot, libraryPath);
  }

  final String _root;
  final String _libraryPath;
  Future<AccessAdminReply>? _pending;
  Future<void>? _closing;
  var _closed = false;

  Future<AccessAdminReply> invoke(AccessAdminRequest request) {
    if (_closed) {
      return Future.error(const AccessAdminException('ADMIN_CLOSED'));
    }
    if (_pending != null) {
      return Future.error(const AccessAdminException('ADMIN_BUSY'));
    }
    final operation = _run(request);
    _pending = operation;
    return operation;
  }

  Future<AccessAdminReply> _run(AccessAdminRequest request) async {
    try {
      return await _dispatch(_root, _libraryPath, request);
    } on AccessAdminException {
      rethrow;
    } catch (_) {
      // A failed worker may have committed; never imply rollback or retry.
      throw AccessAdminException('WORKER_FAILED',
          outcomeUnknown: request.mutation);
    } finally {
      _pending = null;
    }
  }

  /// Drain rather than cancel. Caller timeouts/page disposal cannot stop FFI.
  /// The in-flight caller keeps its original result/error; later calls reject.
  Future<void> close() {
    _closed = true;
    return _closing ??= _drain();
  }

  Future<void> _drain() async {
    final pending = _pending;
    if (pending != null) {
      try {
        await pending;
      } catch (_) {
        // The invoker owns this error; close only joins completion.
      }
    }
  }
}

// Top-level closure scope contains only sendable immutable input, not a client,
// pending Future, library handle or native pointer.
Future<AccessAdminReply> _dispatch(
    String root, String libraryPath, AccessAdminRequest request) =>
    Isolate.run(() {
      final native = NativeAccessAdmin.open(libraryPath);
      final output = native.run(root, AccessAdminCodec.encode(request),
          mutation: request.mutation);
      return AccessAdminCodec.decode(output, request);
    }, debugName: 'wex-access-admin');
