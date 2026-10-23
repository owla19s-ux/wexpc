import 'package:flutter/material.dart';

/// Resource fields and permission intents; the host performs all validation.
class ConversationTools extends StatelessWidget {
  const ConversationTools({
    super.key,
    required this.pathController,
    required this.urlController,
    required this.repository,
    required this.branch,
    required this.busy,
    required this.canRead,
    required this.canPropose,
    required this.browserEnabled,
    required this.onFile,
    required this.onBrowser,
  });

  final TextEditingController pathController;
  final TextEditingController urlController;
  final String? repository;
  final String? branch;
  final bool busy;
  final bool canRead;
  final bool canPropose;
  final bool browserEnabled;
  final void Function(bool read, bool propose) onFile;
  final ValueChanged<bool> onBrowser;

  @override
  Widget build(BuildContext context) => ExpansionTile(
    title: const Text('本对话工具授权'),
    children: [
      Text('${repository ?? '未配置仓库'} · ${branch ?? '未选择分支'}'),
      TextField(
        controller: pathController,
        enabled: !busy,
        decoration: const InputDecoration(
          labelText: '仓库内已有文件路径，如 README.md',
        ),
      ),
      const Text('开启读取后，文件文字会发送给当前 AI 服务商。修改提议仍须逐次审查。'),
      SwitchListTile(
        title: const Text('允许 AI 读取所选文件'),
        value: canRead,
        onChanged: busy
            ? null
            : (v) => onFile(v, canPropose),
      ),
      SwitchListTile(
        title: const Text('允许 AI 提议修改（不会直接写入）'),
        value: canPropose,
        onChanged: busy || !canRead
            ? null
            : (v) => onFile(true, v),
      ),
      TextButton(
        onPressed: busy || !canRead
            ? null
            : () => onFile(true, canPropose),
        child: const Text('应用新文件路径并作废旧提议'),
      ),
      TextField(
        controller: urlController,
        enabled: !busy,
        decoration: const InputDecoration(
          labelText: '指定公开 HTTPS 网页（不带查询参数）',
        ),
      ),
      SwitchListTile(
        title: const Text('允许 AI 只读此网页'),
        subtitle: const Text('匿名、静态文字；不支持搜索、跳转、登录、点击或表单'),
        value: browserEnabled,
        onChanged: busy ? null : onBrowser,
      ),
      TextButton(
        onPressed: busy || !browserEnabled
            ? null
            : () => onBrowser(true),
        child: const Text('应用新网页地址'),
      ),
    ],
  );
}
