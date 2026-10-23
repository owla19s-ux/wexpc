import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:wex/access_admin/access_admin_models.dart';
import 'package:wex/access_admin/access_admin_codec.dart';

Uint8List bytes(Object? value) =>
    Uint8List.fromList(utf8.encode(jsonEncode(value)));
Map<String, Object?> reply(Object? data) => {
  'protocol_version': 1, 'request_id': 'r1', 'status': 'ok',
  'data': data, 'error': null,
};
Matcher failure(String code, {bool unknown = false}) =>
    isA<AccessAdminException>()
        .having((e) => e.code, 'code', code)
        .having((e) => e.outcomeUnknown, 'unknown', unknown);

void main() {
  final list = AccessAdminRequest.list(requestId: 'r1');
  final edit = AccessAdminRequest.set(
      requestId: 'r1', expectedRevision: '0', directoryId: 'docs',
      access: DirectoryAccess.readWrite, explicitPersonal: true);

  test('typed request preserves permission and decimal revision without root', () {
    final value = jsonDecode(utf8.decode(AccessAdminCodec.encode(edit)));
    expect(value, {
      'protocol_version': 1, 'request_id': 'r1',
      'command': {'operation': 'set', 'expected_revision': '0',
        'directory_id': 'docs', 'access': 'read_write',
        'explicit_personal': true},
    });
    expect(jsonDecode(utf8.decode(AccessAdminCodec.encode(
      AccessAdminRequest.revoke(requestId: 'x',
        expectedRevision: '18446744073709551615', directoryId: 'docs'),
    )))['command']['expected_revision'], '18446744073709551615');
  });

  test('invalid identifiers and noncanonical or overflowing revisions reject', () {
    for (final id in ['', 'a/b', '个人', 'a' * 65, 'a\n']) {
      expect(() => AccessAdminRequest.list(requestId: id),
          throwsA(failure('INVALID_REQUEST')));
      expect(() => AccessAdminRequest.revoke(requestId: 'r1',
          expectedRevision: '0', directoryId: id),
          throwsA(failure('INVALID_REQUEST')));
    }
    for (final rev in ['', '00', '01', '-1', '+1', '1.0', '1e2',
      '18446744073709551616', '0\n']) {
      expect(() => AccessAdminRequest.revoke(requestId: 'r1',
          expectedRevision: rev, directoryId: 'docs'),
          throwsA(failure('INVALID_REQUEST')));
    }
  });

  test('list preserves maximum u64 and immutable grant snapshot', () {
    final result = AccessAdminCodec.decode(bytes(reply({
      'revision': '18446744073709551615',
      'grants': [{'directory_id': 'docs', 'access': 'read_write',
        'explicit_personal': true}],
    })), list);
    expect(result.revision, '18446744073709551615');
    expect(result.grants!.single.directoryId, 'docs');
    expect(result.grants!.single.access, DirectoryAccess.readWrite);
    expect(result.grants!.single.explicitPersonal, true);
    expect(() => result.grants!.clear(), throwsUnsupportedError);
    expect(AccessAdminCodec.decode(bytes(reply({'revision': '1'})), edit)
        .grants, isNull);
  });

  test('malformed response never confirms a write', () {
    final samples = <Object?>[
      {}, {...reply({'revision': '1'}), 'extra': true},
      {...reply({'revision': '1'}), 'request_id': 'wrong'},
      {...reply({'revision': '1'}), 'protocol_version': 1.0},
      {...reply({'revision': '1'}), 'error': {'code': 'COMMIT_UNKNOWN'}},
      reply({'revision': 1}), reply({'revision': '01'}),
      reply({'revision': '18446744073709551616'}),
      reply({'revision': '1', 'grants': []}),
    ];
    for (final sample in samples) {
      expect(() => AccessAdminCodec.decode(bytes(sample), edit),
          throwsA(failure('MALFORMED_REPLY', unknown: true)));
    }
  });

  test('strict JSON rejects duplicate escaped keys depth trailing and bad UTF8', () {
    for (final text in [
      '{"protocol_version":1,"protocol_version":1}',
      '{"data":{"revision":"1","revi\\u0073ion":"1"}}',
      '${'[' * 18}0${']' * 18}', '{}{}',
    ]) {
      expect(() => AccessAdminCodec.decode(
          Uint8List.fromList(utf8.encode(text)), list),
          throwsA(failure('MALFORMED_REPLY')));
    }
    for (final input in [
      Uint8List(0), Uint8List(65537), Uint8List.fromList([255]),
    ]) {
      expect(() => AccessAdminCodec.decode(input, edit),
          throwsA(failure('MALFORMED_REPLY', unknown: true)));
    }
  });

  test('grant response enforces exact types unique IDs and 128 maximum', () {
    Map<String, Object?> grant(String id) =>
        {'directory_id': id, 'access': 'read', 'explicit_personal': false};
    final max = List.generate(128, (i) => grant('d$i'));
    expect(AccessAdminCodec.decode(bytes(reply({
      'revision': '0', 'grants': max,
    })), list).grants!.length, 128);
    for (final grants in [
      [...max, grant('overflow')], [grant('same'), grant('same')],
      [{...grant('d'), 'extra': true}],
      [{...grant('d'), 'explicit_personal': 'false'}],
      [{...grant('d'), 'access': 'execute'}], [grant('a/b')],
    ]) {
      expect(() => AccessAdminCodec.decode(bytes(reply({
        'revision': '0', 'grants': grants,
      })), list), throwsA(failure('MALFORMED_REPLY')));
    }
  });

  test('only allowed errors and retry flags accepted with unknown commit', () {
    Map<String, Object?> error(String code, bool retryable) => {
      'protocol_version': 1, 'request_id': 'r1', 'status': 'error',
      'data': null, 'error': {'code': code, 'retryable': retryable},
    };
    expect(() => AccessAdminCodec.decode(
        bytes(error('COMMIT_UNKNOWN', false)), edit),
        throwsA(failure('COMMIT_UNKNOWN', unknown: true)));
    expect(() => AccessAdminCodec.decode(
        bytes(error('STORE_BUSY', true)), list),
        throwsA(isA<AccessAdminException>().having(
            (e) => e.retryable, 'retryable', true)));
    for (final value in [error('raw path', false),
      error('STORE_BUSY', false), error('REVISION_CONFLICT', true)]) {
      expect(() => AccessAdminCodec.decode(bytes(value), edit),
          throwsA(failure('MALFORMED_REPLY', unknown: true)));
    }
  });
}
