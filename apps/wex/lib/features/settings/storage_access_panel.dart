import 'package:flutter/material.dart';
import '../../platform/storage_access_service.dart';

class StorageAccessPanel extends StatefulWidget {
  const StorageAccessPanel({super.key});

  @override
  State<StorageAccessPanel> createState() => _StorageAccessPanelState();
}

class _StorageAccessPanelState extends State<StorageAccessPanel>
    with WidgetsBindingObserver {
  StorageAccessStatus? _status;
  bool _busy = false;
  bool _refreshAgain = false;
  bool _settingsFailed = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _refresh();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) _refresh();
  }

  Future<void> _refresh({bool openSettings = false}) async {
    if (_busy) {
      _refreshAgain = true;
      return;
    }
    setState(() => _busy = true);
    var failed = _settingsFailed;
    if (openSettings) {
      failed = !await StorageAccessService.openSettings();
    }
    final status = await StorageAccessService.status();
    if (!mounted) return;
    setState(() {
      _status = status;
      _busy = false;
      _settingsFailed = failed;
    });
    if (_refreshAgain) {
      _refreshAgain = false;
      _refresh();
    }
  }

  @override
  Widget build(BuildContext context) {
    final status = _status;
    final String label;
    if (status == null) {
      label = '正在检查系统文件权限…';
    } else if (status.error != null) {
      label = '系统文件权限检查失败：${status.error}';
    } else if (!status.supported) {
      label = '当前系统不支持此授权入口';
    } else {
      label = status.granted ? '所有文件访问：已授权' : '所有文件访问：未授权';
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text('APP 系统文件权限'),
        const Text('此权限不会自动授予 AI 目录读写权限，关联或切换空间也不会扩大 AI 权限。'),
        const Text('部分系统受限目录仍不可访问；WEX 日志目录单独选择。'),
        Text(label),
        if (_settingsFailed)
          const Text('无法打开系统设置，请刷新或从系统设置手动进入。'),
        Wrap(
          spacing: 8,
          children: [
            FilledButton.tonal(
              onPressed: _busy || status?.supported != true
                  ? null
                  : () => _refresh(openSettings: true),
              child: const Text('打开系统授权设置'),
            ),
            TextButton(
              onPressed: _busy ? null : () => _refresh(),
              child: const Text('刷新系统权限'),
            ),
          ],
        ),
        const Text('AI 多目录授权与系统权限分离；下方只读检查提供独立的临时授权和撤销回执。'),
      ],
    );
  }
}
