import 'dart:convert';
import 'dart:math';
import 'package:flutter/foundation.dart';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// Android-only extension settings. Credentials are never placed in app logs,
/// widget keys, a repository, or the Flutter/Rust protocol.
class ConnectorConfig {
  const ConnectorConfig({
    this.apiBaseUrl = 'https://api.openai.com/v1',
    this.apiKey = '',
    this.modelId = '',
    this.githubToken = '',
    this.repository = 'owla19s-ux/WEX',
    this.branch = '',
    this.historyScope = '',
    this.aiAvailable = true,
    this.githubAvailable = true,
    this.browserAvailable = true,
  });

  final String apiBaseUrl;
  final String apiKey;
  final String modelId;
  final String githubToken;
  final String repository;
  final String branch;
  /// A random local identity; never contains API keys or account identifiers.
  final String historyScope;
  // Availability is not session/resource consent or write approval.
  final bool aiAvailable, githubAvailable, browserAvailable;

  ConnectorConfig withHistoryScope(String scope) => ConnectorConfig(
    apiBaseUrl: apiBaseUrl, apiKey: apiKey, modelId: modelId,
    githubToken: githubToken, repository: repository, branch: branch,
    historyScope: scope,
    aiAvailable: aiAvailable, githubAvailable: githubAvailable,
    browserAvailable: browserAvailable,
  );

  ConnectorConfig withAvailability({
    bool? aiAvailable, bool? githubAvailable, bool? browserAvailable,
  }) => ConnectorConfig(
    apiBaseUrl: apiBaseUrl, apiKey: apiKey, modelId: modelId,
    githubToken: githubToken, repository: repository, branch: branch,
    historyScope: historyScope,
    aiAvailable: aiAvailable ?? this.aiAvailable,
    githubAvailable: githubAvailable ?? this.githubAvailable,
    browserAvailable: browserAvailable ?? this.browserAvailable,
  );

  bool get hasAi =>
      aiAvailable && apiKey.trim().isNotEmpty && modelId.trim().isNotEmpty;
  bool get hasGitHub =>
      githubAvailable && githubToken.trim().isNotEmpty &&
      repository.trim().isNotEmpty &&
      branch.trim().isNotEmpty;

  /// Reject cleartext custom API endpoints before sending user credentials.
  void validate() {
    final uri = Uri.tryParse(apiBaseUrl.trim());
    if (uri == null ||
        uri.scheme != 'https' ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        uri.hasFragment ||
        uri.hasQuery) {
      throw const FormatException('AI API 地址必须是 HTTPS Base URL（不能带凭证、参数或片段）');
    }
    if (repository.isNotEmpty &&
        !RegExp(r'^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$')
            .hasMatch(repository.trim())) {
      throw const FormatException('仓库格式应为 owner/repo');
    }
    if (branch.contains('..') ||
        branch.contains(' ') ||
        branch.startsWith('/') ||
        branch.endsWith('/') ||
        branch.startsWith('-')) {
      throw const FormatException('分支名称无效');
    }
  }

  Map<String, dynamic> toJson() => {
        'version': 1,
        'apiBaseUrl': apiBaseUrl.trim(),
        'apiKey': apiKey.trim(),
        'modelId': modelId.trim(),
        'githubToken': githubToken.trim(),
        'repository': repository.trim(),
        'branch': branch.trim(),
        'historyScope': historyScope,
        'aiAvailable': aiAvailable,
        'githubAvailable': githubAvailable,
        'browserAvailable': browserAvailable,
      };

  factory ConnectorConfig.fromJson(Map<String, dynamic> data) {
    if (data['version'] != 1) {
      throw const FormatException('不支持的连接配置版本');
    }
    return ConnectorConfig(
      apiBaseUrl: data['apiBaseUrl'] as String? ?? '',
      apiKey: data['apiKey'] as String? ?? '',
      modelId: data['modelId'] as String? ?? '',
      githubToken: data['githubToken'] as String? ?? '',
      repository: data['repository'] as String? ?? '',
      branch: data['branch'] as String? ?? '',
      historyScope: data['historyScope'] as String? ?? '',
      aiAvailable: data['aiAvailable'] as bool? ?? true,
      githubAvailable: data['githubAvailable'] as bool? ?? true,
      browserAvailable: data['browserAvailable'] as bool? ?? true,
    );
  }
}

/// Process-local invalidation metadata. Never contains credentials or targets.
/// Null flags mean unchanged; identity changes keep the existing reload barrier.
class ConnectorConfigChange {
  const ConnectorConfigChange({
    required this.revision, required this.identityChanged,
    this.aiAvailable, this.githubAvailable, this.browserAvailable,
  });
  final int revision;
  final bool identityChanged;
  final bool? aiAvailable, githubAvailable, browserAvailable;
}

class ConnectorConfigStore {
  ConnectorConfigStore({FlutterSecureStorage? storage})
      : _storage = storage ?? const FlutterSecureStorage();

  static const _key = 'wex.android.connectors.v1';
  /// Process-local invalidation; consumers must match metadata to this revision.
  static final ValueNotifier<int> revisions = ValueNotifier<int>(0);
  static ConnectorConfigChange? latestChange;
  final FlutterSecureStorage _storage;

  static String _newScope() => List.generate(
    16, (_) => Random.secure().nextInt(256).toRadixString(16).padLeft(2, '0'),
  ).join();

  Future<ConnectorConfig> load() async {
    final raw = await _storage.read(key: _key);
    if (raw == null || raw.isEmpty) return const ConnectorConfig();
    final config = ConnectorConfig.fromJson(jsonDecode(raw) as Map<String, dynamic>);
    if (config.historyScope.isNotEmpty) return config;
    // Old unscoped histories are deliberately not migrated across providers.
    // A stable opaque scope is generated once for an existing saved connection.
    final migrated = config.withHistoryScope(_newScope());
    await _storage.write(key: _key, value: jsonEncode(migrated.toJson()));
    return migrated;
  }

  Future<void> save(ConnectorConfig config) async {
    config.validate();
    final previous = await load();
    final sameAi = previous.apiBaseUrl.trim() == config.apiBaseUrl.trim() &&
        previous.modelId.trim() == config.modelId.trim() &&
        previous.apiKey.trim() == config.apiKey.trim();
    // Repository/branch/token also identify the confidential tool context.
    final sameResources =
        previous.githubToken.trim() == config.githubToken.trim() &&
        previous.repository.trim() == config.repository.trim() &&
        previous.branch.trim() == config.branch.trim();
    final scope = sameAi && sameResources && previous.historyScope.isNotEmpty
        ? previous.historyScope : _newScope();
    await _storage.write(key: _key, value: jsonEncode(
      config.withHistoryScope(scope).toJson(),
    ));
    final revision = revisions.value + 1;
    latestChange = ConnectorConfigChange(
      revision: revision,
      identityChanged: !sameAi || !sameResources || previous.historyScope != scope,
      aiAvailable: previous.aiAvailable == config.aiAvailable ? null : config.aiAvailable,
      githubAvailable: previous.githubAvailable == config.githubAvailable ? null : config.githubAvailable,
      browserAvailable: previous.browserAvailable == config.browserAvailable ? null : config.browserAvailable,
    );
    revisions.value = revision;
  }

  Future<void> delete() async {
    await _storage.delete(key: _key);
    latestChange = null;
    revisions.value++;
  }
}
