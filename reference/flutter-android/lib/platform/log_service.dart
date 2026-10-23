import 'package:flutter/services.dart';
import 'package:flutter/foundation.dart';

class LogStatus {
  const LogStatus({required this.authorized, required this.saved,
    this.directory, this.error});
  final bool authorized;
  final bool saved;
  final String? directory;
  final String? error;
  factory LogStatus.fromMap(Map<Object?, Object?> value) => LogStatus(
    authorized: value['authorized'] == true, saved: value['saved'] == true,
    directory: value['directory'] as String?, error: value['error'] as String?);
}

/// Platform diagnostics accepts fixed event codes only, never raw exception text.
abstract final class LogService {
  static const _channel = MethodChannel('dev.wex/diagnostics');
  static final latest = ValueNotifier<LogStatus?>(null);
  static const _events = {'FLUTTER_START', 'FLUTTER_ERROR', 'DART_UNCAUGHT',
    'LIBRARY_LOADING', 'LIBRARY_READY', 'LIBRARY_FAILED', 'BRIDGE_START',
    'BRIDGE_READY', 'BRIDGE_FAILED', 'DIAGNOSTIC_CHECK'};
  static const _codes = {'LIBRARY_LOAD_FAILED', 'ABI_MISMATCH', 'NATIVE_INTERNAL',
    'NATIVE_INVALID_ARGUMENT', 'NATIVE_CAPACITY', 'NATIVE_PANIC',
    'MALFORMED_REPLY', 'RUNTIME_BUSY', 'RUNTIME_CLOSED'};

  static Future<void> event(String event, {String? code}) async {
    if (!_events.contains(event)) return;
    try {
      final value = await _channel.invokeMapMethod<Object?, Object?>('event', {
        'event': event, if (_codes.contains(code)) 'code': code,
      })
        .timeout(const Duration(seconds: 3));
      latest.value = LogStatus.fromMap(value ?? {});
    } catch (_) { _unavailable(); }
  }

  static Future<void> exception(String event, StackTrace stack) async {
    if (!_events.contains(event)) return;
    // Only a WEX source location; never the exception message or a full stack.
    final site = RegExp(r'package:wex/([A-Za-z0-9_/]+\.dart:\d+:\d+)')
        .firstMatch(stack.toString())?.group(1);
    try {
      final value = await _channel.invokeMapMethod<Object?, Object?>('event', {
        'event': event, if (site != null) 'site': site,
      }).timeout(const Duration(seconds: 3));
      latest.value = LogStatus.fromMap(value ?? {});
    } catch (_) { _unavailable(); }
  }

  static Future<LogStatus> _call(String method) async {
    try {
      final operation = _channel.invokeMapMethod<Object?, Object?>(method);
      final value = await (method == 'chooseDirectory' ? operation
          : operation.timeout(const Duration(seconds: 10)));
      final status = LogStatus.fromMap(value ?? {});
      latest.value = status;
      return status;
    } catch (_) {
      return _unavailable();
    }
  }
  static LogStatus _unavailable() {
    const status = LogStatus(authorized: false, saved: false,
      error: 'PLATFORM_UNAVAILABLE');
    latest.value = status;
    return status;
  }
  static Future<LogStatus> status() => _call('status');
  static Future<LogStatus> chooseDirectory() => _call('chooseDirectory');
}
