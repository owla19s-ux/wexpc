import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import '../../access_admin/access_admin_models.dart';
import '../../local_read/local_read_exception.dart';
import '../../local_read/local_read_inspector.dart';
import 'local_directory_picker.dart';

/// Trusted settings-only inspection, never an AI tool or Space permission.
class LocalReadInspectionPanel extends StatefulWidget {
  const LocalReadInspectionPanel({super.key, this.openInspector});
  final Future<LocalInspectionSession> Function()? openInspector;
  @override
  State<LocalReadInspectionPanel> createState() => _LocalReadInspectionPanelState();
}

class _LocalReadInspectionPanelState extends State<LocalReadInspectionPanel> {
  final _file = TextEditingController();
  LocalInspectionSession? _inspector;
  String? _path;
  String? _message;
  String? _preview;
  bool _busy = false;
  bool _unsafe = false;

  @override
  void dispose() {
    _file.dispose();
    final inspector = _inspector;
    if (inspector != null) {
      unawaited(inspector.close().catchError((Object _) {}));
    }
    super.dispose();
  }

  String _code(Object error) {
    if (error is LocalInspectionException) return error.code;
    if (error is LocalReadException) return error.code;
    if (error is AccessAdminException) return error.code;
    return 'OPERATION_FAILED';
  }

  Future<void> _perform(Future<void> Function() action) async {
    if (_busy) return;
    setState(() { _busy = true; _message = null; });
    try {
      await action();
    } catch (error) {
      if (!mounted) return;
      final code = _code(error);
      setState(() {
        _message = '操作未完成：$code';
        if (_inspector?.blocked == true) _unsafe = true;
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _choose() async {
    if (_busy || _unsafe) return;
    final path = await showDialog<String>(
        context: context, builder: (_) => const LocalDirectoryPicker());
    if (!mounted || path == null) return;
    await _perform(() async {
      final inspector = _inspector ??= await (widget.openInspector ?? LocalReadInspector.open)();
      await inspector.selectDirectory(path);
      if (!mounted) return;
      setState(() {
        _path = path;
        _preview = null;
        _file.clear();
        _message = '目录已登记；尚未授权读取。';
      });
    });
  }

  Future<void> _read() async {
    final inspector = _inspector;
    if (_busy || _unsafe || inspector == null || _path == null) return;
    final leaf = _file.text;
    final confirmed = await showDialog<bool>(context: context,
      builder: (dialog) => AlertDialog(
        title: const Text('确认本次只读授权'),
        content: const Text('仅为这次人工读取授予所选目录的临时 read 权限，包括明确的个人目录同意。完成读取后尝试立即撤销；不允许写文件，不向 AI 暴露工具。'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(dialog, false),
              child: const Text('取消')),
          FilledButton(onPressed: () => Navigator.pop(dialog, true),
              child: const Text('授权并读取')),
        ],
      ));
    if (!mounted || confirmed != true) return;
    await _perform(() async {
      final bytes = await inspector.readOnce(leaf);
      if (!mounted) return;
      setState(() {
        _preview = _describe(bytes);
        _message = '已读取 ${bytes.length} 字节；临时授权已撤销（设备兼容性仍待真机验收）。';
      });
    });
  }

  Future<void> _revoke() => _perform(() async {
    final inspector = _inspector;
    if (inspector == null) return;
    await inspector.reviewAndRevoke();
    if (!mounted) return;
    setState(() {
      _unsafe = false;
      _preview = null;
      _message = '当前目录会话的授权已核对并撤销。';
    });
  });

  static String _describe(Uint8List bytes) {
    if (bytes.isEmpty) return '空文件（0 字节）';
    try {
      final text = utf8.decode(bytes);
      final prefix = text.length > 2048 ? text.substring(0, 2048) : text;
      return 'UTF-8 文本预览：\n$prefix${text.length > 2048 ? '\n…仅显示前 2048 字符' : ''}';
    } on FormatException {
      final count = bytes.length < 64 ? bytes.length : 64;
      final hex = bytes.take(count).map((b) => b.toRadixString(16).padLeft(2, '0')).join(' ');
      return '二进制文件（仅展示前 $count 字节十六进制）：\n$hex';
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text('本地文件只读检查', style: TextStyle(fontWeight: FontWeight.w600)),
        const Text('与 APP 系统权限分离；Space 关联不会授权。仅支持当前 Android 可直接访问的文件路径，SAF URI 暂不适用。'),
        if (_path != null) SelectableText('当前目录：$_path'),
        if (_busy) const LinearProgressIndicator(),
        Wrap(spacing: 8, children: [
          OutlinedButton(
            onPressed: _busy || _unsafe ? null : _choose,
            child: const Text('选择目录'),
          ),
          if (_unsafe) OutlinedButton(
            onPressed: _busy ? null : _revoke,
            child: const Text('核对并撤销授权'),
          ),
        ]),
        if (_path != null) ...[
          TextField(
            controller: _file,
            enabled: !_busy && !_unsafe,
            key: const ValueKey('local-read-leaf'),
            decoration: const InputDecoration(
              labelText: '直属文件名（不含路径）',
              hintText: 'example.txt',
            ),
          ),
          FilledButton(
            onPressed: _busy || _unsafe ? null : _read,
            key: const ValueKey('local-read-submit'),
            child: const Text('读取文件'),
          ),
        ],
        if (_message != null) Text(_message!, key: const ValueKey('local-read-status')),
        if (_unsafe) const Text('授权状态未确认：禁止继续读取。请显式核对撤销；不要将此状态视为成功。'),
        if (_preview != null)
          SelectableText(_preview!, key: const ValueKey('local-read-preview')),
        const Text('限制：直属普通文件，最多 64 KiB；不持久化目录身份，重启后需重新选择。'),
      ],
    );
  }
}
