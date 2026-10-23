import 'dart:convert';
import 'dart:typed_data';

import '../platform/strict_json.dart';
import 'access_admin_models.dart';

abstract final class AccessAdminCodec {
  static Uint8List encode(AccessAdminRequest request) {
    final command = <String, Object?>{'operation': request.operation};
    if (request.mutation) {
      command.addAll({
        'expected_revision': request.expectedRevision,
        'directory_id': request.directoryId,
      });
    }
    if (request.operation == 'set') {
      command.addAll({
        'access': request.access!.wire,
        'explicit_personal': request.explicitPersonal,
      });
    }
    final bytes = Uint8List.fromList(utf8.encode(jsonEncode({
      'protocol_version': 1,
      'request_id': request.requestId,
      'command': command,
    })));
    if (bytes.isEmpty || bytes.length > 16384) {
      throw const AccessAdminException('INVALID_REQUEST');
    }
    return bytes;
  }

  static Map<String, dynamic> _map(Object? value, List<String> keys) {
    if (value is! Map<String, dynamic> || value.length != keys.length ||
        !keys.every(value.containsKey)) {
      throw const FormatException();
    }
    return value;
  }

  static const _errorCodes = {
    'INVALID_REQUEST', 'UNSUPPORTED_VERSION', 'STORE_UNAVAILABLE',
    'STORE_BUSY', 'INVALID_SNAPSHOT', 'INVALID_GRANT', 'REVISION_CONFLICT',
    'GRANT_LIMIT', 'REVISION_EXHAUSTED', 'COMMIT_UNKNOWN',
  };

  static AccessAdminReply decode(
      Uint8List bytes, AccessAdminRequest request) {
    try {
      final value = _map(decodeStrictJson(bytes),
          ['protocol_version', 'request_id', 'status', 'data', 'error']);
      if (value['protocol_version'] is! int ||
          value['protocol_version'] != 1 ||
          value['request_id'] != request.requestId) {
        throw const FormatException();
      }
      if (value['status'] == 'error') {
        if (value['data'] != null) throw const FormatException();
        final error = _map(value['error'], ['code', 'retryable']);
        final code = error['code'];
        if (!_errorCodes.contains(code) ||
            error['retryable'] != (code == 'STORE_BUSY')) {
          throw const FormatException();
        }
        throw AccessAdminException(code as String,
            outcomeUnknown: request.mutation && code == 'COMMIT_UNKNOWN',
            retryable: code == 'STORE_BUSY');
      }
      if (value['status'] != 'ok' || value['error'] != null) {
        throw const FormatException();
      }
      final data = _map(value['data'], request.mutation
          ? ['revision'] : ['revision', 'grants']);
      if (!validAdminRevision(data['revision'])) {
        throw const FormatException();
      }
      if (request.mutation) {
        return AccessAdminReply(revision: data['revision'] as String);
      }
      final entries = data['grants'];
      if (entries is! List || entries.length > 128) {
        throw const FormatException();
      }
      final ids = <String>{};
      final grants = <AccessAdminGrant>[];
      for (final entry in entries) {
        final grant = _map(entry,
            ['directory_id', 'access', 'explicit_personal']);
        final id = grant['directory_id'];
        if (id is! String || !validAdminId(id) || !ids.add(id) ||
            grant['explicit_personal'] is! bool) {
          throw const FormatException();
        }
        final access = DirectoryAccess.values
            .firstWhere((access) => access.wire == grant['access']);
        grants.add(AccessAdminGrant(directoryId: id, access: access,
            explicitPersonal: grant['explicit_personal'] as bool));
      }
      return AccessAdminReply(
          revision: data['revision'] as String, grants: grants);
    } on AccessAdminException {
      rethrow;
    } catch (_) {
      throw AccessAdminException('MALFORMED_REPLY',
          outcomeUnknown: request.mutation);
    }
  }
}
