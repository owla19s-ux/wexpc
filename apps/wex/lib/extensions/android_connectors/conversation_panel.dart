import 'package:flutter/material.dart';
import 'conversation_controller.dart';
import 'github_tool_session.dart';
import '../../features/conversation/widgets/conversation_message_card.dart';
import '../../features/conversation/widgets/conversation_composer.dart';
import '../../features/conversation/widgets/conversation_tools.dart';
import '../../features/conversation/widgets/github_change_dialog.dart';

export '../../features/conversation/widgets/github_change_dialog.dart' show replacementDiff;

class AndroidConversationPanel extends StatefulWidget {
  const AndroidConversationPanel({
    super.key,
    required this.draft,
    this.controller,
  });
  final TextEditingController draft;
  final AndroidConversationController? controller;
  @override
  State<AndroidConversationPanel> createState() =>
      _AndroidConversationPanelState();
}

class _AndroidConversationPanelState extends State<AndroidConversationPanel> {
  late final _host = widget.controller ?? AndroidConversationController();
  final _path = TextEditingController();
  final _url = TextEditingController();
  @override
  void initState() {
    super.initState();
    _host.addListener(_update);
  }

  void _update() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    _host.removeListener(_update);
    if (widget.controller == null) _host.dispose();
    _path.dispose();
    _url.dispose();
    super.dispose();
  }

  void _file(bool read, bool propose) {
    try {
      _host.selectFile(_path.text, read: read, propose: propose);
    } catch (_) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('请检查 GitHub 配置和仓库内文件路径')));
    }
  }

  void _browser(bool enabled) {
    try {
      _host.selectBrowser(_url.text, enabled);
    } catch (_) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('请确认浏览器已在连接设置启用并加载，且地址为公开 HTTPS 网页')),
      );
    }
  }

  Future<void> _approve(GitHubChangeProposal proposal) async {
    final snapshot = proposal.snapshot;
    final approved = await showDialog<bool>(
      context: context,
      builder: (ctx) => GitHubChangeDialog(
        repository: snapshot.repository,
        branch: snapshot.branch,
        path: snapshot.path,
        blobSha: snapshot.blobSha,
        before: snapshot.text,
        after: proposal.replacement,
      ),
    );
    if (approved == true && mounted) await _host.approve(proposal.id);
  }

  @override
  Widget build(BuildContext context) {
    final c = _host.config;
    final pending = _host.githubSession?.pending;
    return Column(
      children: [
        Expanded(
          child: ListView(
            padding: const EdgeInsets.all(12),
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      c?.hasAi == true ? '模型：${c!.modelId}' : '尚未连接 AI',
                    ),
                  ),
                  TextButton(
                    onPressed: _host.busy ? null : _host.load,
                    child: const Text('加载连接'),
                  ),
                ],
              ),
              const Text('首片：一组连接、一个自动保存的对话（最近 40 条）。完整多对话/分组管理尚未接入。'),
              ConversationTools(
                pathController: _path,
                urlController: _url,
                repository: c?.repository,
                branch: c?.branch,
                busy: _host.busy,
                canRead: _host.canRead,
                canPropose: _host.canPropose,
                browserEnabled: _host.browserEnabled,
                onFile: _file,
                onBrowser: _browser,
              ),
              SelectableText(_host.status),
              if (_host.busy) const LinearProgressIndicator(),
              if (pending != null)
                PendingGitHubChangeCard(
                  path: pending.snapshot.path,
                  onReview: _host.busy ? null : () => _approve(pending),
                  onDiscard: _host.busy ? null : _host.discard,
                ),
              for (final m in _host.entries)
                ConversationMessageCard(role: m.role, text: m.text),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
          child: Align(
            alignment: Alignment.centerLeft,
            child: Text(
              _host.committing
                  ? '提交核验中：不可重复提交或发送'
                  : _host.busy
                      ? '正在处理请求；可停止并撤销工具授权'
                      : pending != null
                          ? '有待审批的 GitHub 修改；请先批准或丢弃'
                          : c?.hasAi != true
                              ? 'AI 未连接：草稿会保留，请先加载连接'
                              : _host.status,
              key: const ValueKey('composer-state'),
              style: Theme.of(context).textTheme.bodySmall,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ),
        ConversationComposer(
          draft: widget.draft,
          busy: _host.busy,
          onStop: _host.committing ? null : _host.stop,
          onSend: c?.hasAi != true
              ? null
              : () {
                  final value = widget.draft.text;
                  final submission = _host.submit(value);
                  if (submission.accepted) widget.draft.clear();
                },
        ),
      ],
    );
  }
}
