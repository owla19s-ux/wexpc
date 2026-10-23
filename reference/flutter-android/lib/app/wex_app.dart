import 'dart:async';
import 'dart:io';
import '../platform/log_service.dart';
import '../runtime/runtime_client.dart';
import '../runtime/isolate_runtime_client.dart';
import 'package:flutter/material.dart';
import '../features/conversation/conversation_page.dart';
import '../features/settings/settings_page.dart';
import '../features/space/space_page.dart';

class WexApp extends StatelessWidget {
  const WexApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'WEX',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF5368C7)),
        scaffoldBackgroundColor: const Color(0xFFF6F7FB),
      ),
      home: const _WorkShell(),
    );
  }
}

class _WorkShell extends StatefulWidget {
  const _WorkShell();

  @override
  State<_WorkShell> createState() => _WorkShellState();
}

class _WorkShellState extends State<_WorkShell> {
  int _selected = 0;
  final _draft = TextEditingController();
  Future<RuntimeClient>? _runtime;
  Future<RuntimeClient> _getRuntime() => _runtime ??= _openRuntime();

  Future<RuntimeClient> _openRuntime() async {
    unawaited(LogService.event('LIBRARY_LOADING'));
    try {
      final client = await PlatformRuntimeClient.open();
      unawaited(LogService.event('LIBRARY_READY'));
      return client;
    } on RuntimeTransportException catch (e) {
      unawaited(LogService.event('LIBRARY_FAILED', code: e.code));
      rethrow;
    } catch (_) {
      unawaited(LogService.event('LIBRARY_FAILED'));
      rethrow;
    }
  }

  @override
  void initState() {
    super.initState();
    if (Platform.isAndroid) {
      unawaited(_checkStartupBridge());
      WidgetsBinding.instance.addPostFrameCallback((_) async {
        final status = await LogService.status();
        if (!mounted || status.saved) return;
        await showDialog<void>(context: context, builder: (dialogContext) => AlertDialog(
          title: const Text('选择 WEX 日志目录'),
          content: const Text('授权一个可在文件管理器查看的目录，启动与错误日志将自动写入。也可稍后在设置中授权；此时仅保留应急记录。'),
          actions: [
            TextButton(onPressed: () => Navigator.pop(dialogContext), child: const Text('稍后设置')),
            FilledButton(onPressed: () async {
              Navigator.pop(dialogContext);
              await LogService.chooseDirectory();
              if (mounted) setState(() => _selected = 2);
            }, child: const Text('选择目录')),
          ],
        ));
      });
    }
  }

  Future<void> _checkStartupBridge() async {
    await LogService.event('BRIDGE_START');
    try {
      final client = await _getRuntime();
      final reply = await client.invoke(RuntimeRequest.ping(requestId: 'startup-ping'));
      await LogService.event(reply.value == 'pong' ? 'BRIDGE_READY' : 'BRIDGE_FAILED');
    } catch (_) {
      await LogService.event('BRIDGE_FAILED');
    }
  }

  @override
  void dispose() {
    final runtime = _runtime;
    if (runtime != null) {
      unawaited(runtime.then<void>((client) => client.close(), onError:(Object e, StackTrace st) {}));
    }
    _draft.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    const titles = ['空间', '对话', '设置'];
    return PopScope<void>(
      canPop: _selected == 0,
      onPopInvokedWithResult: (didPop, result) {
        if (!didPop && _selected != 0) {
          setState(() => _selected = 0);
        }
      },
      child: Scaffold(
        appBar: AppBar(title: Text(titles[_selected])),
        body: SafeArea(
          child: IndexedStack(
            index: _selected,
            children: [
              const SpacePage(),
              ConversationPage(draft: _draft),
              SettingsPage(runtime:_getRuntime),
            ],
          ),
        ),
        bottomNavigationBar: NavigationBar(
          selectedIndex: _selected,
          onDestinationSelected: (index) => setState(() => _selected = index),
          destinations: const [
            NavigationDestination(icon: Icon(Icons.workspaces_outline), label: '空间'),
            NavigationDestination(icon: Icon(Icons.chat_bubble_outline), label: '对话'),
            NavigationDestination(icon: Icon(Icons.settings_outlined), label: '设置'),
          ],
        ),
      ),
    );
  }
}

