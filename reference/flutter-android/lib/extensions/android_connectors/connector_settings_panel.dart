import 'package:flutter/material.dart';
import 'connector_config.dart';
import 'connector_diagnostics.dart';
import 'github_target_picker.dart';

class ConnectorSettingsPanel extends StatefulWidget {
  const ConnectorSettingsPanel({super.key});
  @override
  State<ConnectorSettingsPanel> createState() => _ConnectorSettingsPanelState();
}

class _ConnectorSettingsPanelState extends State<ConnectorSettingsPanel> {
  final _store = ConnectorConfigStore();
  final _diagnostics = ConnectorDiagnostics();
  final _browserTestUrl = TextEditingController();
  final _fields = List.generate(6, (_) => TextEditingController());
  final _available = [true, true, true];
  final _status = List.filled(3, '正在读取安全配置…');
  bool _busy = true;
  bool _loaded = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    // A failed reload must not leave stale forms enabled for saving.
    setState(() { _busy = true; _loaded = false; });
    try {
      final c = await _store.load();
      if (!mounted) return;
      final values = [c.apiBaseUrl, c.apiKey, c.modelId,
        c.githubToken, c.repository, c.branch];
      for (var i = 0; i < values.length; i++) {
        _fields[i].text = values[i];
      }
      _available[0] = c.aiAvailable;
      _available[1] = c.githubAvailable;
      _available[2] = c.browserAvailable;
      _loaded = true;
      for (var i = 0; i < _status.length; i++) {
        _status[i] = '已读取配置；尚未进行连接测试。';
      }
    } catch (_) {
      for (var i = 0; i < _status.length; i++) {
        _status[i] = '配置读取失败，禁止保存；请重试。';
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _save(int section) async {
    setState(() => _busy = true);
    try {
      // Merge only this section with fresh persisted data, never other drafts.
      final c = await _store.load();
      if (!mounted) return;
      await _store.save(_snapshot(c, section));
      _status[section] = '已保存，未进行连接测试。身份或目标变更需重新加载；启停立即同步，对应工具需重新授权。';
    } on FormatException catch (e) {
      _status[section] = e.message;
    } catch (_) {
      _status[section] = '保存失败，结果未确认；请重新加载核对。';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }


  ConnectorConfig _snapshot(ConnectorConfig c, int section) => ConnectorConfig(
        apiBaseUrl: section == 0 ? _fields[0].text : c.apiBaseUrl,
        apiKey: section == 0 ? _fields[1].text : c.apiKey,
        modelId: section == 0 ? _fields[2].text : c.modelId,
        githubToken: section == 1 ? _fields[3].text : c.githubToken,
        repository: section == 1 ? _fields[4].text : c.repository,
        branch: section == 1 ? _fields[5].text : c.branch,
        aiAvailable: section == 0 ? _available[0] : c.aiAvailable,
        githubAvailable: section == 1 ? _available[1] : c.githubAvailable,
        browserAvailable: section == 2 ? _available[2] : c.browserAvailable,
      );

  void _edited(int section) {
    if (_loaded && !_busy) {
      setState(() => _status[section] = '配置已编辑；旧测试结果已过期，尚未保存。');
    }
  }

  Future<void> _test(int section) async {
    setState(() {
      _busy = true;
      _status[section] = '正在测试当前表单；不会保存或授予会话权限…';
    });
    try {
      final stored = await _store.load();
      if (!mounted) return;
      final c = _snapshot(stored, section);
      if (section == 0) {
        await _diagnostics.testAi(c);
      } else if (section == 1) {
        await _diagnostics.testGitHub(c);
      } else {
        if (!c.browserAvailable) throw StateError('浏览器未启用');
        await _diagnostics.testBrowser(_browserTestUrl.text.trim());
      }
      _status[section] = const [
        '实际请求收到文本回复；可能计费。未保存配置，未授权工具。',
        '仓库只读访问成功；未验证分支或写入权限，未保存配置。',
        '指定网页读取到文字；未保存地址，未向 AI 发送网页或授予对话权限。',
      ][section];
    } catch (_) {
      // Never surface raw provider errors: they may echo credentials or URLs.
      _status[section] = '连接测试失败；请检查启用状态、地址、凭证、模型或网络。未自动重试。';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _pickGitHubTarget() async {
    setState(() => _busy = true);
    try {
      final stored = await _store.load();
      if (!mounted) return;
      final selection = await showDialog<GitHubTarget>(
        context: context,
        builder: (_) => GitHubTargetPicker(config: _snapshot(stored, 1)),
      );
      if (selection != null && mounted) {
        _fields[4].text = selection.repository;
        _fields[5].text = selection.branch;
        _status[1] = '目标已选入草稿；请保存后在对话重新加载并授权。旧测试结果已过期。';
      }
    } catch (_) {
      _status[1] = '读取配置失败；未改变目标。';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    for (final f in _fields) { f.dispose(); }
    _browserTestUrl.dispose();
    _diagnostics.dispose();
    super.dispose();
  }

  Widget _field(int i) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 6),
    child: TextField(
      key: ValueKey('connector-field-$i'),
      controller: _fields[i],
      onChanged: (_) => _edited(i < 3 ? 0 : 1),
      enabled: !_busy && _loaded,
      obscureText: i == 1 || i == 3,
      autocorrect: false,
      enableSuggestions: false,
      decoration: InputDecoration(
        border: const OutlineInputBorder(),
        labelText: const ['AI Base URL', 'AI API Key', 'Model ID',
          'GitHub Token', 'GitHub owner/repo', 'GitHub 工作分支'][i],
      ),
    ),
  );

  Widget _section(int i, String title, String description, List<int> fields) =>
    ExpansionTile(
      key: PageStorageKey('connector-section-$i'),
      maintainState: true,
      title: Text(title),
      childrenPadding: const EdgeInsets.only(bottom: 12),
      children: [
        Text(description),
        SwitchListTile(
          title: Text('启用$title'),
          subtitle: const Text('保存后生效；启用不代表授予会话工具权限'),
          value: _available[i],
          onChanged: _busy || !_loaded ? null :
            (value) {
              setState(() => _available[i] = value);
              _edited(i);
            },
        ),
        for (final field in fields) _field(field),
        if (i == 1)
          OutlinedButton(
            onPressed: _busy || !_loaded || !_available[1] ? null : _pickGitHubTarget,
            child: const Text('选择仓库与分支'),
          ),
        if (i == 2)
          TextField(
            key: const ValueKey('browser-test-url'),
            controller: _browserTestUrl,
            enabled: !_busy && _loaded,
            onChanged: (_) => _edited(2),
            decoration: const InputDecoration(labelText: '测试公开 HTTPS 网页（仅本次）'),
          ),
        Text(_status[i]),
        if (i == 0) const Text('测试会发送一次真实模型请求，可能产生费用。'),
        OutlinedButton(
          onPressed: _busy || !_loaded ? null : () => _test(i),
          child: Text(const ['测试 AI 实际请求', '测试仓库只读访问', '测试指定网页读取'][i]),
        ),
        FilledButton(
          onPressed: _busy || !_loaded ? null : () => _save(i),
          child: Text('保存 $title'),
        ),
      ],
    );

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      const SizedBox(height: 12),
      const Text('Android 连接配置'),
      _section(0, 'AI API',
        'OpenAI-compatible；Base URL 需包含版本路径。可独立配置并用于普通对话。',
        [0, 1, 2]),
      _section(1, 'GitHub',
        '凭证保存在本机安全存储。当前适配仍使用选定仓库/分支与已有文件；'
        '已有文件暂限24KiB。可选任意凭证可访问的仓库/分支；GitHub 分支保护仍生效。读取/提议在对话中授权，提交仍需批准实际差异。',
        [3, 4, 5]),
      _section(2, 'AI 浏览器',
        '当前适配：匿名读取人指定的公开 HTTPS 静态网页。无需 API Key；'
        '不使用登录态，不支持搜索、点击或表单。网页地址与本次读取授权在对话中选择。',
        []),
      const Text('配置安全保存；保存不等于连接测试或操作成功。'
        '启停只影响对应模块；开启工具仍须对话授权。凭证、模型或仓库目标变更会撤销旧权限并要求重新加载。'),
      TextButton(
        onPressed: _busy ? null : _load,
        child: const Text('重新加载全部配置（覆盖未保存编辑）'),
      ),
    ],
  );
}
