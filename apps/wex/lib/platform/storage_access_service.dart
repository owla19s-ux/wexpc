import 'package:flutter/services.dart';

class StorageAccessStatus {
  const StorageAccessStatus({
    required this.supported,
    required this.granted,
    this.error,
  });

  final bool supported;
  final bool granted;
  final String? error;

  factory StorageAccessStatus.fromMap(Map<Object?, Object?> value) {
    final supported = value['supported'];
    final granted = value['granted'];
    final error = value['error'];
    if (supported is! bool ||
        granted is! bool ||
        (!supported && granted) ||
        (error != null && error is! String)) {
      return const StorageAccessStatus(
        supported: false,
        granted: false,
        error: 'MALFORMED_STATUS',
      );
    }
    return StorageAccessStatus(
      supported: supported,
      granted: granted && error == null,
      error: error as String?,
    );
  }
}

/// System capability only. Never creates or changes an AI directory grant.
abstract final class StorageAccessService {
  static const _channel = MethodChannel('dev.wex/storage_access');

  static Future<StorageAccessStatus> status() async {
    try {
      final value = await _channel
          .invokeMapMethod<Object?, Object?>('status')
          .timeout(const Duration(seconds: 5));
      return StorageAccessStatus.fromMap(value ?? {});
    } catch (_) {
      return const StorageAccessStatus(
        supported: false,
        granted: false,
        error: 'PLATFORM_UNAVAILABLE',
      );
    }
  }

  /// True means a system page opened, never that permission was granted.
  static Future<bool> openSettings() async {
    try {
      return await _channel
              .invokeMethod<bool>('openSettings')
              .timeout(const Duration(seconds: 5)) ==
          true;
    } catch (_) {
      return false;
    }
  }
}
