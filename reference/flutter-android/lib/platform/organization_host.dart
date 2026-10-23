import 'dart:convert';

import 'package:flutter/services.dart';

/// Trusted local metadata storage only. Does not grant resource access.
/// This endpoint is never used by AI tools, Context addresses or user paths.
abstract final class OrganizationHost {
  static const _channel = MethodChannel('dev.wex/organization_host');

  static Future<String> privateRoot() async {
    try {
      final root = await _channel.invokeMethod<Object?>('privateRoot');
      if (root is! String ||
          !root.startsWith('/') ||
          root.contains('\u0000') ||
          utf8.encode(root).length > 4096 ||
          !root.endsWith('/files/organization-state') ||
          root.split('/').skip(1).any(
              (p) => p.isEmpty || p == '.' || p == '..')) {
        throw const OrganizationHostException();
      }
      return root;
    } catch (_) {
      // Never leak a private root, plugin detail or fallback path.
      throw const OrganizationHostException();
    }
  }
}

final class OrganizationHostException implements Exception {
  const OrganizationHostException();

  @override
  String toString() => 'OrganizationHostException(HOST_UNAVAILABLE)';
}
