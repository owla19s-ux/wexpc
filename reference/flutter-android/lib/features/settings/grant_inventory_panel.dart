import 'dart:async';

import 'package:flutter/material.dart';

import '../../access_admin/access_admin_models.dart';
import '../../access_admin/grant_inventory_client.dart';

/// Explicitly user-driven recovery of opaque Rust grant records.
/// It neither registers directories nor changes Android / AI access switches.
class GrantInventoryPanel extends StatefulWidget {
  const GrantInventoryPanel({super.key, this.openSession});
  final Future<GrantInventorySession> Function()? openSession;

  @override
  State<GrantInventoryPanel> createState() => _GrantInventoryPanelState();
}

class _GrantInventoryPanelState extends State<GrantInventoryPanel> {
  GrantInventorySession? _session;
  AccessAdminReply? _snapshot;
  String? _message;
  bool _busy = false;

  @override
  void dispose() {
    final session = _session;
    if (session != null) {
      unawaited(session.close().catchError((Object _) {}));
    }
    super.dispose();
  }

  String _error(Object error) {
    if (error is AccessAdminException) {
      return error.outcomeUnknown
          ? '${error.code}（写入结果未知）'
          : error.code;
    }
    return 'ADMIN_UNAVAILABLE';
  }

  Future<GrantInventorySession> _getSession() async {
    final existing = _session;
    if (existing != null) return existing;
    final created = await (widget.openSession ?? GrantInventoryClient.open)();
    if (!mounted) {
      unawaited(created.close().catchError((Object _) {}));
      throw const AccessAdminException('ADMIN_CLOSED');
    }
    _session = created;
    return created;
  }

  Future<void> _load() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      final session = await _getSession();
      final snapshot = await session.list();
      if (!mounted) return;
      if (snapshot.grants == null) {
        throw const AccessAdminException('MALFORMED_REPLY');
      }
      setState(() => _snapshot = snapshot);
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _snapshot = null;
        _message = '读取授权记录失败：${_error(error)}';
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _remove(AccessAdminGrant record) async {
    if (_busy || _snapshot == null || _session == null) return;
    final original = _snapshot!;
    if (!original.grants!.any((item) =>
        item.directoryId == record.directoryId)) {
      return;
    }
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      final accepted = await showDialog<bool>(
        context: context,
        builder: (dialog) => AlertDialog(
          title: const Text('确认撤销授权记录'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Text('仅删除这一条 Rust 授权记录，不删除文件、不改变 Android 系统权限，也不会恢复旧目录连接。若其他会话正在使用该记录，后续读取可能被拒绝。'),
              const SizedBox(height: 8),
              SelectableText(record.directoryId),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(dialog, false),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(dialog, true),
              child: const Text('确认撤销'),
            ),
          ],
        ),
      );
      if (!mounted || accepted != true) return;
      // The displayed revision is the CAS precondition. Do not silently
      // refresh it before writing; concurrent changes require a human review.
      await _session!.revoke(record.directoryId, original.revision);
      final verified = await _session!.list();
      if (!mounted) return;
      if (verified.grants == null ||
          verified.grants!.any((item) =>
              item.directoryId == record.directoryId)) {
        throw const AccessAdminException('REVOKE_NOT_VERIFIED');
      }
      setState(() {
        _snapshot = verified;
        _message = '已从授权存储撤销并通过重新读取核验；不代表系统文件权限已改变。';
      });
    } catch (error) {
      if (!mounted) return;
      setState(() {
        // No optimistic removal or automatic retry, including COMMIT_UNKNOWN.
        _snapshot = null;
        _message = '撤销未能核验：${_error(error)}。请手动刷新授权记录后再判断。';
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  String _accessLabel(DirectoryAccess access) => switch (access) {
    DirectoryAccess.read => '只读',
    DirectoryAccess.write => '只写',
    DirectoryAccess.readWrite => '读写',
  };

  @override
  Widget build(BuildContext context) {
    final snapshot = _snapshot;
    final grants = snapshot?.grants;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text('目录授权记录',
            style: TextStyle(fontWeight: FontWeight.w600)),
        const Text('仅显示本机 Rust 授权存储中的匿名目录 ID。旧会话可能已失效，无法从 ID 还原文件路径；刷新不会创建权限。'),
        TextButton.icon(
          key: const ValueKey('grant-inventory-refresh'),
          onPressed: _busy ? null : _load,
          icon: const Icon(Icons.refresh),
          label: Text(snapshot == null ? '查看授权记录' : '刷新授权记录'),
        ),
        if (_busy) const LinearProgressIndicator(),
        if (_message != null)
          Text(_message!, key: const ValueKey('grant-inventory-status')),
        if (grants != null) ...[
          Text('当前授权记录：${grants.length} 条'),
          if (grants.isEmpty) const Text('暂无目录授权记录'),
          if (grants.isNotEmpty)
            SizedBox(
              height: 260,
              child: ListView.builder(
                itemCount: grants.length,
                itemBuilder: (context, index) {
                  final record = grants[index];
                  final id = record.directoryId;
                  final shownId = id.length > 16
                      ? '${id.substring(0, 16)}…'
                      : id;
                  return ListTile(
                    dense: true,
                    key: ValueKey('grant-record-$id'),
                    title: Text('目录 ID：$shownId'),
                    subtitle: Text(
                      '${_accessLabel(record.access)} · 个人目录明确授权：${record.explicitPersonal ? '是' : '否'}'),
                    trailing: TextButton(
                      key: ValueKey('grant-revoke-$id'),
                      onPressed: _busy ? null : () => _remove(record),
                      child: const Text('撤销'),
                    ),
                  );
                },
              ),
            ),
        ],
      ],
    );
  }
}
