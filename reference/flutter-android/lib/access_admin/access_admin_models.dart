enum DirectoryAccess {
  read('read'),
  write('write'),
  readWrite('read_write');

  const DirectoryAccess(this.wire);
  final String wire;
}

bool validAdminId(String value) =>
    value.isNotEmpty && value.length <= 64 &&
    value.codeUnits.every((c) =>
        (c >= 65 && c <= 90) || (c >= 97 && c <= 122) ||
        (c >= 48 && c <= 57) || c == 95 || c == 45);

bool validAdminRevision(Object? value) {
  if (value is! String || value.isEmpty || value.length > 20 ||
      (value.length > 1 && value.startsWith('0')) ||
      !value.codeUnits.every((c) => c >= 48 && c <= 57)) {
    return false;
  }
  return BigInt.parse(value) <= BigInt.parse('18446744073709551615');
}

/// A trusted user edit, not an AI tool or a binding to a real directory.
/// Revision strings preserve u64 exactly across the bridge.
final class AccessAdminRequest {
  const AccessAdminRequest._(this.requestId, this.operation,
      this.expectedRevision, this.directoryId, this.access,
      this.explicitPersonal);

  factory AccessAdminRequest.list({required String requestId}) {
    _check(requestId);
    return AccessAdminRequest._(requestId, 'list', null, null, null, null);
  }

  factory AccessAdminRequest.set({
    required String requestId,
    required String expectedRevision,
    required String directoryId,
    required DirectoryAccess access,
    required bool explicitPersonal,
  }) {
    _check(requestId, expectedRevision, directoryId);
    return AccessAdminRequest._(requestId, 'set', expectedRevision,
        directoryId, access, explicitPersonal);
  }

  factory AccessAdminRequest.revoke({
    required String requestId,
    required String expectedRevision,
    required String directoryId,
  }) {
    _check(requestId, expectedRevision, directoryId);
    return AccessAdminRequest._(requestId, 'revoke', expectedRevision,
        directoryId, null, null);
  }

  static void _check(String id, [String? revision, String? directory]) {
    if (!validAdminId(id) ||
        (revision != null && !validAdminRevision(revision)) ||
        (directory != null && !validAdminId(directory))) {
      throw const AccessAdminException('INVALID_REQUEST');
    }
  }

  final String requestId;
  final String operation;
  final String? expectedRevision;
  final String? directoryId;
  final DirectoryAccess? access;
  final bool? explicitPersonal;
  bool get mutation => operation != 'list';
}

final class AccessAdminGrant {
  const AccessAdminGrant({
    required this.directoryId,
    required this.access,
    required this.explicitPersonal,
  });
  final String directoryId;
  final DirectoryAccess access;
  final bool explicitPersonal;
}

final class AccessAdminReply {
  AccessAdminReply({required this.revision, List<AccessAdminGrant>? grants})
      : grants = grants == null ? null : List.unmodifiable(grants);
  final String revision;
  /// Non-null for list only. A UI snapshot is never a file-execution token.
  final List<AccessAdminGrant>? grants;
}

final class AccessAdminException implements Exception {
  const AccessAdminException(this.code, {
    this.outcomeUnknown = false,
    this.retryable = false,
  });
  final String code;
  final bool outcomeUnknown;
  /// Informational only; the client never automatically retries.
  final bool retryable;
  @override
  String toString() => 'AccessAdminException($code)';
}
