import 'dart:convert';

import 'package:flutter/services.dart';

/// Trusted composition only: never expose this root as an AI/Space parameter.
/// Resolves a location; does not initialize a store or grant resource access.
abstract final class AccessAdminHost {
  static const _channel = MethodChannel('dev.wex/access_admin_host');

  static Future<String> privateRoot() async {
    try {
      final value = await _channel.invokeMethod<Object?>('privateRoot');
      if (value is! String ||
          !_validAbsolutePath(value) ||
          value.split('/').last != 'access-grants') {
        throw const AccessAdminHostException();
      }
      return value;
    } catch (_) {
      // No raw platform exception, path, fallback, cache or automatic retry.
      throw const AccessAdminHostException();
    }
  }

  /// Complete trusted internal layout, not a grant or a resource identifier.
  /// No legacy-root fallback: one root cannot stand in for a missing layout.
  static Future<AccessPrivateLayout> privateLayout() async {
    try {
      final value = await _channel.invokeMethod<Object?>('privateLayout');
      if (value is! Map ||
          value.length != 3 ||
          !value.containsKey('version') ||
          !value.containsKey('privateRoot') ||
          !value.containsKey('internalRoots')) {
        throw const AccessAdminHostException();
      }
      final version = value['version'];
      final rawRoots = value['internalRoots'];
      if (version is! int ||
          version != 1 ||
          rawRoots is! List ||
          rawRoots.isEmpty ||
          rawRoots.length > 2) {
        throw const AccessAdminHostException();
      }
      final roots = <String>[];
      for (final root in rawRoots) {
        if (root is! String ||
            !_validAbsolutePath(root) ||
            roots.contains(root)) {
          throw const AccessAdminHostException();
        }
        roots.add(root);
      }
      final root = value['privateRoot'];
      if (root is! String ||
          !_validAbsolutePath(root) ||
          root != '${roots.first}/files/access-grants') {
        throw const AccessAdminHostException();
      }
      return AccessPrivateLayout._(root, roots);
    } catch (_) {
      throw const AccessAdminHostException();
    }
  }

  static bool _validAbsolutePath(String value) {
    if (!value.startsWith('/') ||
        value.contains('\u0000') ||
        utf8.encode(value).length > 4096) {
      return false;
    }
    return !value.split('/').skip(1).any(
        (part) => part.isEmpty || part == '.' || part == '..');
  }
}

/// Immutable trusted Host configuration; does not prove filesystem availability.
final class AccessPrivateLayout {
  AccessPrivateLayout._(this.privateRoot, List<String> roots)
      : internalRoots = List<String>.unmodifiable(roots);

  final String privateRoot;
  final List<String> internalRoots;
}

class AccessAdminHostException implements Exception {
  const AccessAdminHostException();

  @override
  String toString() => 'AccessAdminHostException(HOST_UNAVAILABLE)';
}
