import 'access_admin_client.dart';
import 'access_admin_models.dart';

/// Trusted local settings only. A record is an opaque grant ID, not a path,
/// live directory registration, Android permission, or Context association.
abstract interface class GrantInventorySession {
  Future<AccessAdminReply> list();
  Future<AccessAdminReply> revoke(String directoryId, String expectedRevision);
  Future<void> close();
}

/// Reuses the existing Rust Admin ABI. Never grants access, guesses a path,
/// retries an unknown mutation, or silently removes other records.
final class GrantInventoryClient implements GrantInventorySession {
  GrantInventoryClient.forAdmin(this._admin);

  static Future<GrantInventoryClient> open() async =>
      GrantInventoryClient.forAdmin(await AccessAdminClient.open());

  final AccessAdminClient _admin;
  int _requestNumber = 0;

  String _id() => 'inventory-${++_requestNumber}';

  @override
  Future<AccessAdminReply> list() =>
      _admin.invoke(AccessAdminRequest.list(requestId: _id()));

  @override
  Future<AccessAdminReply> revoke(
      String directoryId, String expectedRevision) =>
      _admin.invoke(AccessAdminRequest.revoke(
          requestId: _id(),
          expectedRevision: expectedRevision,
          directoryId: directoryId));

  @override
  Future<void> close() => _admin.close();
}
