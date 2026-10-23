import 'dart:convert';
import 'dart:typed_data';

import '../access_admin/access_admin_client.dart';
import '../access_admin/access_admin_models.dart';
import 'local_read_client.dart';
import 'local_read_exception.dart';

class LocalInspectionException implements Exception {
  const LocalInspectionException(this.code, {this.outcomeUnknown = false});
  final String code;
  final bool outcomeUnknown;
}

/// A trusted, foreground-only manual read. Registration is NOT an AI grant.
/// Persistent grant changes are explicit and bracket one single file read.
abstract interface class LocalInspectionSession {
  bool get blocked;
  Future<void> selectDirectory(String path);
  Future<Uint8List> readOnce(String leaf);
  Future<void> reviewAndRevoke();
  Future<void> close();
}

final class LocalReadInspector implements LocalInspectionSession {
  LocalReadInspector(this._reader, this._admin);
  final LocalReadClient _reader;
  final AccessAdminClient _admin;
  String? _directoryId;
  bool _busy = false;
  bool _blocked = false;
  bool _closed = false;
  int _sequence = 0;

  bool get blocked => _blocked;
  bool get hasDirectory => _directoryId != null;
  String _requestId() => 'inspect-${++_sequence}';

  static Future<LocalReadInspector> open() async {
    final reader = await LocalReadClient.open();
    try {
      final admin = await AccessAdminClient.open();
      return LocalReadInspector(reader, admin);
    } catch (_) {
      await reader.close();
      rethrow;
    }
  }

  Future<T> _exclusive<T>(Future<T> Function() action) async {
    if (_closed) throw const LocalInspectionException('INSPECTOR_CLOSED');
    if (_busy) throw const LocalInspectionException('INSPECTOR_BUSY');
    if (_blocked) throw const LocalInspectionException('GRANT_REVIEW_REQUIRED');
    _busy = true;
    try {
      return await action();
    } finally {
      _busy = false;
    }
  }

  Future<void> selectDirectory(String path) => _exclusive(() async {
    if (!path.startsWith('/') || utf8.encode(path).length > 4096 ||
        path.contains('\u0000') || path.split('/').skip(1).any(
            (p) => p.isEmpty || p == '.' || p == '..')) {
      throw const LocalInspectionException('INVALID_DIRECTORY');
    }
    final id = await _reader.register(path);
    _directoryId = id;
  });

  static bool _validLeaf(String leaf) {
    final bytes = utf8.encode(leaf);
    return bytes.isNotEmpty && bytes.length <= 255 && leaf != '.' &&
        leaf != '..' && !leaf.contains('/') && !leaf.contains('\u0000') &&
        !leaf.runes.any((c) => c < 32 || c == 127);
  }

  Future<Uint8List> readOnce(String leaf) => _exclusive(() async {
    final id = _directoryId;
    if (id == null) throw const LocalInspectionException('NO_DIRECTORY');
    // Reject before creating even a temporary grant.
    if (!_validLeaf(leaf)) throw const LocalInspectionException('INVALID_FILE');
    final snapshot = await _admin.invoke(
        AccessAdminRequest.list(requestId: _requestId()));
    if (snapshot.grants!.any((grant) => grant.directoryId == id)) {
      _blocked = true;
      throw const LocalInspectionException('GRANT_REVIEW_REQUIRED');
    }
    try {
      await _admin.invoke(AccessAdminRequest.set(
          requestId: _requestId(), expectedRevision: snapshot.revision,
          directoryId: id, access: DirectoryAccess.read,
          explicitPersonal: true));
    } on AccessAdminException catch (error) {
      if (error.outcomeUnknown) _blocked = true;
      rethrow;
    }
    Uint8List? bytes;
    Object? readFailure;
    try {
      bytes = await _reader.read(id, leaf);
    } catch (error) {
      readFailure = error;
    }
    // Always independently check live grant state before revoking. A failed
    // revoke hides file content and blocks future reads; never claim cleanup.
    try {
      await _revokeId(id);
    } catch (_) {
      _blocked = true;
      throw const LocalInspectionException('REVOKE_UNCONFIRMED',
          outcomeUnknown: true);
    }
    if (readFailure != null) {
      if (readFailure is LocalReadException) throw readFailure;
      throw const LocalInspectionException('READ_FAILED');
    }
    return bytes!;
  });

  Future<void> _revokeId(String id) async {
    final snapshot = await _admin.invoke(
        AccessAdminRequest.list(requestId: _requestId()));
    if (!snapshot.grants!.any((grant) => grant.directoryId == id)) return;
    await _admin.invoke(AccessAdminRequest.revoke(
        requestId: _requestId(), expectedRevision: snapshot.revision,
        directoryId: id));
  }

  /// Manual recovery is explicitly invoked by the user, never an auto-retry
  /// of an unknown commit. A session may have lost its ID after process death.
  Future<void> reviewAndRevoke() async {
    if (_closed) throw const LocalInspectionException('INSPECTOR_CLOSED');
    if (_busy) throw const LocalInspectionException('INSPECTOR_BUSY');
    final id = _directoryId;
    if (id == null) throw const LocalInspectionException('NO_DIRECTORY');
    _busy = true;
    try {
      await _revokeId(id);
      _blocked = false;
    } catch (_) {
      _blocked = true;
      throw const LocalInspectionException('REVOKE_UNCONFIRMED',
          outcomeUnknown: true);
    } finally {
      _busy = false;
    }
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    await _admin.close();
    await _reader.close();
  }
}
