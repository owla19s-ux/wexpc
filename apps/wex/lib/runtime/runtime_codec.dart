part of 'runtime_models.dart';

abstract final class RuntimeCodec {
  static Uint8List encode(RuntimeRequest request) {
    final op = switch (request.operation) {
      RuntimeOperation.ping => 'ping',
      RuntimeOperation.echo => 'echo',
      RuntimeOperation.demoFail => 'demo_fail',
      RuntimeOperation.demoDeny => 'demo_deny',
    };
    final encoded = Uint8List.fromList(utf8.encode(jsonEncode({
      'protocol_version':1, 'request_id':request.requestId, 'operation':op,
      'payload':request.operation == RuntimeOperation.echo ? {'text':request.echoText} : <String,Object?>{},
    })));
    if (encoded.isEmpty || encoded.length > 16384) {
      throw const RuntimeTransportException('NATIVE_INVALID_ARGUMENT');
    }
    return encoded;
  }

  static bool _keys(Object? value, List<String> keys) =>
      value is Map<String,dynamic> &&
      value.length == keys.length && keys.every(value.containsKey);

  static RuntimeReply decode(Uint8List bytes, RuntimeRequest request) {
    try {
      if (bytes.isEmpty || bytes.length > 65536) throw const FormatException();
      final value = decodeStrictJson(bytes);
      if (!_keys(value, ['protocol_version','request_id','status','data','error'])) throw const FormatException();
      final m = value as Map<String,dynamic>;
      if (m['protocol_version'] is! int || m['protocol_version'] != 1 ||
          m['request_id'] != request.requestId) throw const FormatException();
      if (m['status'] == 'ok') {
        if (!_keys(m['data'], ['value']) || m['error'] != null ||
            m['data']['value'] is! String) throw const FormatException();
        final result = m['data']['value'] as String;
        final expected = switch (request.operation) {
          RuntimeOperation.ping => 'pong',
          RuntimeOperation.echo => request.echoText,
          _ => null,
        };
        if (expected == null || result != expected) throw const FormatException();
        return RuntimeReply._(request.requestId, RuntimeStatus.ok, result, null);
      }
      if (m['status'] != 'error' && m['status'] != 'denied') throw const FormatException();
      if (m['data'] != null || !_keys(m['error'], ['code','message','retryable'])) throw const FormatException();
      final error = m['error'] as Map<String,dynamic>;
      final code = error['code'];
      if (code is! String || !RegExp(r'^[A-Z0-9_]+$').hasMatch(code) ||
          error['message'] is! String || (error['message'] as String).isEmpty ||
          error['retryable'] != false) throw const FormatException();
      final denied = m['status'] == 'denied';
      final protocolError = {'INVALID_REQUEST','UNSUPPORTED_VERSION','UNKNOWN_OPERATION'}.contains(code);
      final valid = denied
          ? request.operation == RuntimeOperation.demoDeny && code == 'DEMO_DENIED'
          : protocolError || (request.operation == RuntimeOperation.demoFail && code == 'DEMO_FAILURE');
      if (!valid) throw const FormatException();
      return RuntimeReply._(request.requestId,
          denied ? RuntimeStatus.denied : RuntimeStatus.error, null,
          RuntimeError._(code, error['message'] as String, false));
    } catch (_) {
      throw const RuntimeTransportException('MALFORMED_REPLY');
    }
  }
}
