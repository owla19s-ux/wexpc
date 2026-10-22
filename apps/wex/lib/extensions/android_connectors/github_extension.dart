import 'dart:convert';

import 'package:github/github.dart';

import 'connector_config.dart';

class RepoFileSnapshot {
  const RepoFileSnapshot({
    required this.repository,
    required this.branch,
    required this.path,
    required this.blobSha,
    required this.text,
  });

  final String repository;
  final String branch;
  final String path;
  final String blobSha;
  final String text;
}

class GitHubWriteReceipt {
  const GitHubWriteReceipt({
    required this.commitSha,
    required this.newBlobSha,
    required this.verified,
  });

  final String commitSha;
  final String newBlobSha;
  final bool verified;
}

/// Android-only GitHub SDK adapter; never clone local repos or use git CLI.
class AndroidGitHubExtension {
  const AndroidGitHubExtension({this.createClient});

  final GitHub Function(String token)? createClient;

  GitHub _client(String token) => createClient?.call(token) ??
      GitHub(auth: Authentication.withToken(token));

  static const maxFileBytes = 24 * 1024;

  static void validatePath(String path) {
    if (path.isEmpty ||
        path.length > 240 ||
        path.startsWith('/') ||
        path.endsWith('/') ||
        path.contains('\\') ||
        path.split('/').any((part) =>
            part.isEmpty || part == '.' || part == '..' || part == '.git')) {
      throw const FormatException('文件路径必须是仓库内的普通相对路径');
    }
  }

  RepositorySlug _slug(ConnectorConfig config, {bool requireBranch = true}) {
    config.validate();
    if (!config.githubAvailable || config.githubToken.trim().isEmpty ||
        config.repository.trim().isEmpty ||
        (requireBranch && config.branch.trim().isEmpty)) {
      throw StateError('请配置 GitHub Token、仓库和工作分支');
    }
    final parts = config.repository.trim().split('/');
    return RepositorySlug(parts[0], parts[1]);
  }

  static const maxCatalogItems = 100;

  /// Human settings inventory; listing never grants conversation access.
  Future<List<String>> listRepositories(ConnectorConfig config) async {
    if (!config.githubAvailable || config.githubToken.trim().isEmpty) {
      throw StateError('请启用 GitHub 并填写 Token');
    }
    final github = _client(config.githubToken);
    try {
      final items = await github.repositories.listRepositories(type: 'all')
          .take(maxCatalogItems).toList().timeout(const Duration(seconds: 30));
      return items.map((item) => item.fullName)
          .where((name) => RegExp(r'^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$').hasMatch(name))
          .toSet().toList();
    } finally {
      github.dispose();
    }
  }

  Future<List<String>> listBranches(ConnectorConfig config) async {
    final slug = _slug(config, requireBranch: false);
    final github = _client(config.githubToken);
    try {
      final items = await github.repositories.listBranches(slug)
          .take(maxCatalogItems).toList().timeout(const Duration(seconds: 30));
      return items.map((item) => item.name ?? '')
          .where((name) => name.isNotEmpty).toSet().toList();
    } finally {
      github.dispose();
    }
  }

  /// Read-only repository visibility check; does not prove branch/write access.
  Future<void> checkConnection(ConnectorConfig config) async {
    final slug = _slug(config, requireBranch: false);
    final github = _client(config.githubToken);
    try {
      await github.repositories.getRepository(slug);
    } finally {
      github.dispose();
    }
  }

  Future<RepoFileSnapshot> read(
    ConnectorConfig config,
    String path,
  ) async {
    validatePath(path);
    final slug = _slug(config);
    final github = _client(config.githubToken);
    try {
      final content = await github.repositories.getContents(
        slug,
        path,
        ref: config.branch,
      );
      final file = content.file;
      if (!content.isFile || file == null) {
        throw StateError('该路径不是常规文件');
      }
      if (file.encoding != 'base64' ||
          file.size == null ||
          file.size! > maxFileBytes ||
          file.sha == null ||
          file.sha!.isEmpty ||
          file.content == null) {
        throw StateError('文件过大、非文本或缺少可验证的 Blob SHA');
      }
      final text = file.text;
      if (utf8.encode(text).length > maxFileBytes) {
        throw StateError('文件无法安全解码为 UTF-8 文本');
      }
      return RepoFileSnapshot(
        repository: config.repository.trim(),
        branch: config.branch.trim(),
        path: path,
        blobSha: file.sha!,
        text: text,
      );
    } finally {
      github.dispose();
    }
  }

  /// No automatic retry. The blob SHA is a GitHub compare-and-swap guard.
  /// The UI must explicitly approve the exact replacement before calling.
  Future<GitHubWriteReceipt> commitApprovedReplacement(
    ConnectorConfig config,
    RepoFileSnapshot snapshot,
    String replacement,
  ) async {
    final slug = _slug(config);
    validatePath(snapshot.path);
    if (snapshot.repository != config.repository.trim() ||
        snapshot.branch != config.branch.trim()) {
      throw StateError('连接配置变化，必须重新读取文件并审查');
    }
    if (replacement == snapshot.text) {
      throw StateError('文件内容没有变化');
    }
    if (utf8.encode(replacement).length > maxFileBytes) {
      throw StateError('修改后的文件超过首版 24KiB 安全上限');
    }

    final github = _client(config.githubToken);
    try {
      // Branch protection is enforced by GitHub, not by branch naming.
      final result = await github.repositories.updateFile(
        slug,
        snapshot.path,
        'WEX approved edit: ${snapshot.path}',
        base64Encode(utf8.encode(replacement)),
        snapshot.blobSha,
        branch: snapshot.branch,
      );
      final sha = result.commit?.sha;
      if (sha == null || sha.isEmpty) {
        throw StateError('GitHub 写入返回结果不完整：状态未知，请人工检查提交记录');
      }
      // Verify this exact commit: the branch may already point to a later edit.
      // Verification failure means UNKNOWN, not an automatic retry.
      final fetched = await github.repositories.getContents(
        slug,
        snapshot.path,
        ref: sha,
      );
      if (!fetched.isFile || fetched.file?.text != replacement) {
        throw StateError('GitHub 返回 Commit $sha，但回读不匹配：须人工核查');
      }
      return GitHubWriteReceipt(
        commitSha: sha,
        newBlobSha: fetched.file?.sha ?? '',
        verified: true,
      );
    } finally {
      github.dispose();
    }
  }
}

