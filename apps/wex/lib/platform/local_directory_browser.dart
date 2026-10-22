import 'dart:convert';
import 'package:flutter/services.dart';

class LocalDirectoryBrowserException implements Exception {
  const LocalDirectoryBrowserException(this.code);
  final String code;
}

final class LocalDirectoryEntry {
  const LocalDirectoryEntry(this.name, this.path);
  final String name;
  final String path;
}

final class LocalDirectoryListing {
  LocalDirectoryListing({
    required this.root,
    required this.current,
    required this.parent,
    required List<LocalDirectoryEntry> directories,
    required this.truncated,
  }) : directories = List.unmodifiable(directories);
  final String root;
  final String current;
  final String? parent;
  final List<LocalDirectoryEntry> directories;
  final bool truncated;

  static bool _path(String value) =>
      value.startsWith('/') &&
      utf8.encode(value).length <= 4096 &&
      !value.contains('\u0000') &&
      !value.split('/').skip(1).any((p) => p.isEmpty || p == '.' || p == '..');

  static LocalDirectoryListing decode(Object? response) {
    if (response is! Map ||
        response.length != 5 ||
        !['root','current','parent','directories','truncated']
            .every(response.containsKey)) {
      throw const LocalDirectoryBrowserException('INVALID_LISTING');
    }
    final root = response['root'];
    final current = response['current'];
    final parent = response['parent'];
    final entries = response['directories'];
    final truncated = response['truncated'];
    if (root is! String || !_path(root) ||
        current is! String || !_path(current) ||
        (current != root && !current.startsWith('$root/')) ||
        (parent != null && (parent is! String || !_path(parent))) ||
        (parent == null && current != root) ||
        (parent != null && (current == root ||
            parent != (current.substring(0, current.lastIndexOf('/'))))) ||
        entries is! List || entries.length > 128 || truncated is! bool) {
      throw const LocalDirectoryBrowserException('INVALID_LISTING');
    }
    final names = <String>{};
    final children = <LocalDirectoryEntry>[];
    for (final entry in entries) {
      if (entry is! Map || entry.length != 2 ||
          !entry.containsKey('name') || !entry.containsKey('path')) {
        throw const LocalDirectoryBrowserException('INVALID_LISTING');
      }
      final name = entry['name'];
      final path = entry['path'];
      if (name is! String || name.isEmpty || name == '.' || name == '..' ||
          name.contains('/') || utf8.encode(name).length > 255 ||
          path is! String || !_path(path) || path != '$current/$name' ||
          !names.add(name)) {
        throw const LocalDirectoryBrowserException('INVALID_LISTING');
      }
      children.add(LocalDirectoryEntry(name, path));
    }
    return LocalDirectoryListing(root: root, current: current,
        parent: parent as String?, directories: children, truncated: truncated);
  }
}

/// Native direct-path browsing requires Android all-files access.
/// SAF content URIs must use their own future adapter, never path conversion.
abstract final class LocalDirectoryBrowser {
  static const channel = MethodChannel('dev.wex/local_directory_browser');

  static Future<LocalDirectoryListing> browse([String? path]) async {
    try {
      final raw = await channel.invokeMethod<Object?>('browse', {'path': path});
      return LocalDirectoryListing.decode(raw);
    } on LocalDirectoryBrowserException {
      rethrow;
    } on PlatformException catch (error) {
      throw LocalDirectoryBrowserException(error.code == 'PERMISSION_REQUIRED'
          ? 'PERMISSION_REQUIRED' : 'BROWSER_UNAVAILABLE');
    } catch (_) {
      throw const LocalDirectoryBrowserException('BROWSER_UNAVAILABLE');
    }
  }
}
