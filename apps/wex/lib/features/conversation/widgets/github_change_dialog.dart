import 'package:flutter/material.dart';

/// A view of the exact immutable proposal selected by the trusted host.
class GitHubChangeDialog extends StatelessWidget {
  const GitHubChangeDialog({
    super.key,
    required this.repository,
    required this.branch,
    required this.path,
    required this.blobSha,
    required this.before,
    required this.after,
  });
  final String repository;
  final String branch;
  final String path;
  final String blobSha;
  final String before;
  final String after;

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('审查本次 GitHub 修改'),
    content: SizedBox(
      width: double.maxFinite,
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SelectableText(
              '$repository\n分支：$branch\n文件：$path\n原 Blob：$blobSha',
            ),
            const SizedBox(height: 12),
            const Text('差异（− 删除，+ 新增）'),
            SelectableText(
              replacementDiff(before, after),
              style: const TextStyle(fontFamily: 'monospace'),
            ),
            const SizedBox(height: 12),
            const Text('批准将向上方分支提交这份准确修改；此审批仅能使用一次。'),
          ],
        ),
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context, false),
        child: const Text('暂不提交'),
      ),
      FilledButton(
        onPressed: () => Navigator.pop(context, true),
        child: const Text('批准并提交'),
      ),
    ],
  );
}

class PendingGitHubChangeCard extends StatelessWidget {
  const PendingGitHubChangeCard({
    super.key, required this.path, required this.onReview, required this.onDiscard,
  });
  final String path;
  final VoidCallback? onReview;
  final VoidCallback? onDiscard;

  @override
  Widget build(BuildContext context) => Card(
    child: Padding(
      padding: const EdgeInsets.all(12),
      child: Column(
        children: [
          Text('待审批：$path（尚未提交）'),
          FilledButton(onPressed: onReview, child: const Text('查看差异并审批')),
          TextButton(onPressed: onDiscard, child: const Text('丢弃提议')),
        ],
      ),
    ),
  );
}

/// Prefix/suffix trimming keeps every changed line visible, including blank
/// lines and final newline changes. No model-supplied diff is trusted.
String replacementDiff(String before, String after) {
  final a = before.split('\n'), b = after.split('\n');
  var start = 0, endA = a.length, endB = b.length;
  while (start < endA && start < endB && a[start] == b[start]) {
    start++;
  }
  while (endA > start && endB > start && a[endA - 1] == b[endB - 1]) {
    endA--;
    endB--;
  }
  return [
    '原文 ${a.length} 行 / 新文 ${b.length} 行；共同前缀 $start 行',
    for (final line in a.sublist(start, endA)) '- $line',
    for (final line in b.sublist(start, endB)) '+ $line',
  ].join('\n');
}
