import 'package:flutter/material.dart';
import '../../platform/log_service.dart';
import '../../runtime/runtime_client.dart';

class RuntimeDiagnostics extends StatefulWidget {
  const RuntimeDiagnostics({super.key, required this.runtime});
  final Future<RuntimeClient> Function() runtime;
  @override
  State<RuntimeDiagnostics> createState() => _RuntimeDiagnosticsState();
}

class _RuntimeDiagnosticsState extends State<RuntimeDiagnostics> {
  bool _running = false;
  String? _result;
  Future<void> _check() async {
    setState(() { _running = true; _result = null; });
    String result;
    await LogService.event('BRIDGE_START');
    try {
      final client = await widget.runtime();
      final ping = await client.invoke(RuntimeRequest.ping(requestId:'check-ping'));
      final echo = await client.invoke(RuntimeRequest.echo(requestId:'check-echo',text:'WEX你好🌙\u0000'));
      final failure = await client.invoke(RuntimeRequest.demoFail(requestId:'check-fail'));
      final denied = await client.invoke(RuntimeRequest.demoDeny(requestId:'check-deny'));
      if (ping.value != 'pong' || echo.value != 'WEX你好🌙\u0000' ||
          failure.status != RuntimeStatus.error || failure.error?.code != 'DEMO_FAILURE' ||
          denied.status != RuntimeStatus.denied || denied.error?.code != 'DEMO_DENIED') {
        throw const RuntimeTransportException('MALFORMED_REPLY');
      }
      result = '本地检查通过：成功、失败与拒绝回执均已收到。';
      await LogService.event('BRIDGE_READY');
    } on RuntimeTransportException catch (e) {
      result = '本地检查失败：${e.code}';
      await LogService.event('BRIDGE_FAILED', code: e.code);
    } catch (_) {
      result = '本地检查失败：NATIVE_INTERNAL';
      await LogService.event('BRIDGE_FAILED');
    }
    if (!mounted) return;
    setState(() { _running = false; _result = result; });
  }
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment:CrossAxisAlignment.start,
    children:[
      const Text('仅检查本地运行时，不连接 AI；拒绝为演示回执。'),
      const SizedBox(height:8),
      FilledButton.tonal(
        key:const ValueKey('runtime-check'),
        onPressed:_running?null:_check,
        child:Text(_running?'检查中…':'检查本地运行时'),
      ),
      if(_result != null) Text(_result!,key:const ValueKey('runtime-check-result')),
    ],
  );
}

