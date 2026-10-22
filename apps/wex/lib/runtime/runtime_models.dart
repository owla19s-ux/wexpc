import 'dart:convert';
import 'dart:typed_data';
import '../platform/strict_json.dart';
part 'runtime_codec.dart';

enum RuntimeOperation { ping, echo, demoFail, demoDeny }
enum RuntimeStatus { ok, error, denied }

final class RuntimeRequest {
  const RuntimeRequest._(this.requestId, this.operation, this.echoText);
  factory RuntimeRequest.ping({required String requestId}) =>
      RuntimeRequest._checked(requestId, RuntimeOperation.ping, null);
  factory RuntimeRequest.echo({required String requestId, required String text}) =>
      RuntimeRequest._checked(requestId, RuntimeOperation.echo, text);
  factory RuntimeRequest.demoFail({required String requestId}) =>
      RuntimeRequest._checked(requestId, RuntimeOperation.demoFail, null);
  factory RuntimeRequest.demoDeny({required String requestId}) =>
      RuntimeRequest._checked(requestId, RuntimeOperation.demoDeny, null);
  static RuntimeRequest _checked(String id, RuntimeOperation op, String? text) {
    if (!RegExp(r'^[A-Za-z0-9_-]{1,64}$').hasMatch(id) ||
        (text != null && utf8.encode(text).length > 4096)) {
      throw ArgumentError('Invalid runtime request');
    }
    return RuntimeRequest._(id, op, text);
  }
  final String requestId;
  final RuntimeOperation operation;
  final String? echoText;
  int get protocolVersion => 1;
}

final class RuntimeReply {
  const RuntimeReply._(this.requestId, this.status, this.value, this.error);
  final String requestId;
  final RuntimeStatus status;
  final String? value;
  final RuntimeError? error;
  int get protocolVersion => 1;
}

final class RuntimeError {
  const RuntimeError._(this.code, this.message, this.retryable);
  final String code;
  final String message;
  final bool retryable;
}

final class RuntimeTransportException implements Exception {
  const RuntimeTransportException(this.code);
  final String code;
  @override
  String toString() => 'Runtime transport: $code';
}
