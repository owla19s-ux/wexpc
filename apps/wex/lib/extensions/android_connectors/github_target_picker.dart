import 'package:flutter/material.dart';
import 'connector_config.dart';
import 'github_extension.dart';

class GitHubTarget {
  const GitHubTarget(this.repository, this.branch);
  final String repository;
  final String branch;
}

/// A human picks a draft target; this dialog never saves or grants permission.
class GitHubTargetPicker extends StatefulWidget {
  const GitHubTargetPicker({super.key, required this.config, this.extension});
  final ConnectorConfig config;
  final AndroidGitHubExtension? extension;
  @override
  State<GitHubTargetPicker> createState() => _GitHubTargetPickerState();
}

class _GitHubTargetPickerState extends State<GitHubTargetPicker> {
  late final _extension = widget.extension ?? const AndroidGitHubExtension();
  List<String> _repositories = [], _branches = [];
  String? _repository;
  bool _busy = true;
  bool _closed = false;
  String _status = '正在读取凭证允许访问的仓库…';

  @override
  void initState() {
    super.initState();
    _loadRepositories();
  }

  Future<void> _loadRepositories() async {
    try {
      final values = await _extension.listRepositories(widget.config);
      if (!mounted || _closed) return;
      _repositories = values;
      _status = values.isEmpty ? '未返回仓库；可取消后手动填写。' : '请选择仓库';
    } catch (_) {
      _status = '仓库列表读取失败；请检查 Token 和网络，或取消后手动填写。';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _loadBranches(String repository) async {
    setState(() {
      _repository = repository;
      _branches = [];
      _busy = true;
      _status = '正在读取分支…';
    });
    try {
      final config = ConnectorConfig.fromJson({
        ...widget.config.toJson(), 'repository': repository, 'branch': '',
      });
      final values = await _extension.listBranches(config);
      if (!mounted || _closed) return;
      _branches = values;
      _status = values.isEmpty ? '未返回分支；空仓库可能尚无分支，可手动填写。' : '请选择分支';
    } catch (_) {
      _status = '分支读取失败；请返回重选，或取消后手动填写。';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget _targetTile(String item, String? expectedRepository) => ListTile(
    title: Text(item),
    onTap: _busy ? null : () {
      // A callback may outlive the frame or the repository list it came from.
      if (_busy || _closed || _repository != expectedRepository) return;
      if (expectedRepository == null) {
        _loadBranches(item);
      } else {
        _close(GitHubTarget(expectedRepository, item));
      }
    },
  );

  void _close([GitHubTarget? target]) {
    if (_closed) return;
    _closed = true;
    Navigator.of(context).pop(target);
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: Text(_repository ?? '选择 GitHub 仓库'),
    content: SizedBox(
      width: 360,
      height: 420,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('只更新设置草稿，不保存或授权。每级最多显示前 100 项；未列出可手动填写。'),
          Text(_status),
          if (_busy) const LinearProgressIndicator(),
          Expanded(
            child: ListView(
              children: [
                for (final item in _repository == null ? _repositories : _branches)
                  _targetTile(item, _repository),
              ],
            ),
          ),
        ],
      ),
    ),
    actions: [
      if (_repository != null)
        TextButton(
          onPressed: _busy ? null : () => setState(() {
            _repository = null;
            _branches = [];
            _status = '请选择仓库';
          }),
          child: const Text('返回仓库列表'),
        ),
      TextButton(
        onPressed: () => _close(),
        child: const Text('取消'),
      ),
    ],
  );
}
