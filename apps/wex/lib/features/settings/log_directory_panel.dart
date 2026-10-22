import 'package:flutter/material.dart';
import '../../platform/log_service.dart';

class LogDirectoryPanel extends StatefulWidget {
  const LogDirectoryPanel({super.key});
  @override
  State<LogDirectoryPanel> createState() => _LogDirectoryPanelState();
}

class _LogDirectoryPanelState extends State<LogDirectoryPanel>
    with WidgetsBindingObserver {
  bool _busy = false;
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
    if (state == AppLifecycleState.resumed && !_busy) _refresh();
  }
  Future<void> _refresh({bool choose = false}) async {
    if (_busy) return;
    setState(() => _busy = true);
    await (choose ? LogService.chooseDirectory() : LogService.status());
    if (!mounted) return;
    setState(() => _busy = false);
  }
  @override
  Widget build(BuildContext context) => ValueListenableBuilder<LogStatus?>(
    valueListenable: LogService.latest,
    builder: (context, status, child) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      const Text('WEX 文件与日志目录（独立于空间工作目录）'),
      const Text('请选择或新建文件管理器可访问的 WEX 文件夹。日志自动写入 wex-diagnostics.log；不授权其他资源。'),
      Text(status == null ? '正在检查日志…' : status.saved
        ? '共享日志已写入' : '共享日志未保存：${status.error ?? "UNKNOWN"}；应急记录状态需随错误检查'),
      if (status?.directory != null) SelectableText(status!.directory!),
      Wrap(spacing: 8, children: [
        FilledButton.tonal(onPressed: _busy ? null : () => _refresh(choose: true),
          child: const Text('选择 WEX 目录')),
        TextButton(onPressed: _busy ? null : () => _refresh(), child: const Text('刷新并补写日志')),
      ]),
    ],
    ),
  );
}
