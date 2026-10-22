import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/runtime/runtime_models.dart';

Matcher transport(String code) => isA<RuntimeTransportException>().having((e) => e.code, 'code', code);
Uint8List bytes(Object value) => Uint8List.fromList(utf8.encode(jsonEncode(value)));
Map<String, Object?> ok(String value) => {
  'protocol_version':1,'request_id':'r1','status':'ok','data':{'value':value},'error':null,
};

void main() {
  final ping = RuntimeRequest.ping(requestId: 'r1');
  test('request factory rejects invalid identifiers and oversized UTF8', () {
    for (final id in ['', '中文', 'a b', 'a' * 65]) {
      expect(() => RuntimeRequest.ping(requestId:id), throwsArgumentError);
    }
    expect(() => RuntimeRequest.echo(requestId:'r1', text:'月' * 1366), throwsArgumentError);
  });
  test('codec encodes protocol and preserves Unicode and NUL', () {
    final request = RuntimeRequest.echo(requestId:'r1', text:'你好🌙\u0000');
    expect(jsonDecode(utf8.decode(RuntimeCodec.encode(request))), {
      'protocol_version':1,'request_id':'r1','operation':'echo','payload':{'text':'你好🌙\u0000'},
    });
    expect(RuntimeCodec.decode(bytes(ok(request.echoText!)), request).value, request.echoText);
  });
  test('final encoded byte bound includes JSON escaping expansion', () {
    expect(RuntimeCodec.encode(RuntimeRequest.echo(requestId:'r1', text:'\u0000' * 2048)).length, lessThanOrEqualTo(16384));
    expect(() => RuntimeCodec.encode(RuntimeRequest.echo(requestId:'r1', text:'\u0000' * 4096)), throwsA(transport('NATIVE_INVALID_ARGUMENT')));
  });
  test('valid ping and demo business errors remain structured results', () {
    expect(RuntimeCodec.decode(bytes(ok('pong')), ping).status, RuntimeStatus.ok);
    for (final denied in [false, true]) {
      final req = denied ? RuntimeRequest.demoDeny(requestId:'r1') : RuntimeRequest.demoFail(requestId:'r1');
      final reply = RuntimeCodec.decode(bytes({
        'protocol_version':1,'request_id':'r1','status':denied?'denied':'error','data':null,
        'error':{'code':denied?'DEMO_DENIED':'DEMO_FAILURE','message':'Demo result','retryable':false},
      }), req);
      expect(reply.status, denied?RuntimeStatus.denied:RuntimeStatus.error);
      expect(reply.error!.retryable, false);
    }
  });
  test('malformed response shapes versions correlation and values reject', () {
    for (final value in [
      {...ok('pong'), 'protocol_version':1.0},
      {...ok('pong'), 'request_id':null},
      {...ok('pong'), 'request_id':'other'},
      {...ok('pong'), 'extra':1},
      {...ok('pong'), 'status':'future'},
      {...ok('pong'), 'data':{'value':'pong','extra':1}},
      {...ok('pong'), 'error':{}},
      ok('wrong pong'),
    ]) {
      expect(() => RuntimeCodec.decode(bytes(value), ping), throwsA(transport('MALFORMED_REPLY')));
    }
  });
  test('duplicate and escaped-equivalent keys invalid UTF8 and depth reject', () {
    final valid = jsonEncode(ok('pong'));
    final duplicates = [
      valid.replaceFirst('"status":"ok"', '"status":"error","status":"ok"'),
      valid.replaceFirst('"value":"pong"', '"value":"x","va\\u006cue":"pong"'),
      valid + ' {}',
      '{"extra":' + '[' * 20 + '0' + ']' * 20 + '}',
    ];
    for (final value in duplicates) {
      expect(() => RuntimeCodec.decode(Uint8List.fromList(utf8.encode(value)), ping), throwsA(transport('MALFORMED_REPLY')));
    }
    expect(() => RuntimeCodec.decode(Uint8List.fromList([255]), ping), throwsA(transport('MALFORMED_REPLY')));
  });
}
