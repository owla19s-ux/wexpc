import 'dart:async';
import 'dart:convert';
import 'package:dart_openai/dart_openai.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';
import 'package:wex/extensions/android_connectors/github_tool_session.dart';

const snapshot = RepoFileSnapshot(repository: 'owner/repo', branch: 'work/test',
  path: 'a.txt', blobSha: 'original-sha', text: 'old');
OpenAIResponseToolCall call(String id, String name, Map<String, Object> args) =>
  OpenAIResponseToolCall.fromMap({'id': id, 'type': 'function',
    'function': {'name': name, 'arguments': jsonEncode(args)}});

void main() {
  test('tools are disabled until user enables the selected file', () async {
    var reads = 0;
    final session = GitHubToolSession(
      repository: 'owner/repo', branch: 'work/test', path: 'a.txt',
      read: () async { reads++; return snapshot; },
      commit: (_, __) async => const GitHubWriteReceipt(
        commitSha: 'commit', newBlobSha: 'blob', verified: true),
    );
    expect(session.tools, isEmpty);
    final reply = await session.handle(call('c1', 'github_read_file', {}));
    expect(reply.toMap()['tool_call_id'], 'c1');
    expect(reply.toMap().toString(), contains('DENIED'));
    expect(reads, 0);
  });

  test('SDK tool proposal never writes and approval is single use', () async {
    var writes = 0;
    final session = GitHubToolSession(
      repository: 'owner/repo', branch: 'work/test', path: 'a.txt',
      read: () async => snapshot,
      commit: (file, text) async {
        writes++; expect(file.blobSha, 'original-sha'); expect(text, 'new');
        return const GitHubWriteReceipt(
          commitSha: 'commit', newBlobSha: 'blob', verified: true);
      },
    );
    session.setPermissions(read: true, propose: true);
    await session.handle(call('read', 'github_read_file', {}));
    await session.handle(call('propose', 'github_propose_replacement',
      {'original_sha': 'original-sha', 'replacement': 'new'}));
    expect(writes, 0);
    expect(session.pending!.replacement, 'new');
    final id = session.pending!.id;
    await session.approve(id);
    expect(writes, 1);
    await expectLater(session.approve(id), throwsStateError);
    expect(writes, 1);
  });

  test('revocation invalidates pending approval', () async {
    var writes = 0;
    final session = GitHubToolSession(
      repository: 'owner/repo', branch: 'work/test', path: 'a.txt',
      read: () async => snapshot,
      commit: (_, __) async { writes++; throw StateError('unexpected'); },
    );
    session.setPermissions(read: true, propose: true);
    await session.handle(call('read', 'github_read_file', {}));
    await session.handle(call('propose', 'github_propose_replacement',
      {'original_sha': 'original-sha', 'replacement': 'new'}));
    final id = session.pending!.id;
    session.setPermissions(read: false, propose: false);
    await expectLater(session.approve(id), throwsStateError);
    expect(writes, 0);
  });

  test('unknown write outcome consumes approval without retry', () async {
    var writes = 0;
    final session = GitHubToolSession(
      repository: 'owner/repo', branch: 'work/test', path: 'a.txt',
      read: () async => snapshot,
      commit: (_, __) async { writes++; throw StateError('unknown'); },
    );
    session.setPermissions(read: true, propose: true);
    await session.handle(call('read', 'github_read_file', {}));
    await session.handle(call('propose', 'github_propose_replacement',
      {'original_sha': 'original-sha', 'replacement': 'new'}));
    final id = session.pending!.id;
    await expectLater(session.approve(id), throwsStateError);
    await expectLater(session.approve(id), throwsStateError);
    expect(writes, 1);
  });
  test('revocation during read prevents late content disclosure', () async {
    final read = Completer<RepoFileSnapshot>();
    final session = GitHubToolSession(repository: 'owner/repo', branch: 'work/test',
      path: 'a.txt', read: () => read.future,
      commit: (_, __) async => throw StateError('unexpected'));
    session.setPermissions(read: true, propose: true);
    final result = session.handle(call('read', 'github_read_file', {}));
    session.setPermissions(read: false, propose: false);
    read.complete(snapshot);
    final reply = await result;
    expect(reply.toMap().toString(), contains('DENIED'));
    expect(reply.toMap().toString(), isNot(contains('original-sha')));
    expect(session.pending, isNull);
  });

  test('model cannot select another target or invoke a commit tool', () async {
    var reads = 0, writes = 0;
    final session = GitHubToolSession(repository: 'owner/repo', branch: 'work/test',
      path: 'a.txt', read: () async { reads++; return snapshot; },
      commit: (_, __) async { writes++; throw StateError('unexpected'); });
    session.setPermissions(read: true, propose: true);
    expect(session.tools.length, 2);
    final badTarget = await session.handle(call('target', 'github_read_file',
      {'path': 'other.txt'}));
    expect(badTarget.toMap().toString(), contains('INVALID_ARGUMENTS'));
    final commit = await session.handle(call('commit', 'github_commit', {}));
    expect(commit.toMap().toString(), contains('UNSUPPORTED'));
    expect(reads, 0); expect(writes, 0);
  });

  test('duplicate argument keys and repeated calls are rejected', () async {
    var reads = 0;
    final session = GitHubToolSession(repository: 'owner/repo', branch: 'work/test',
      path: 'a.txt', read: () async { reads++; return snapshot; },
      commit: (_, __) async => throw StateError('unexpected'));
    session.setPermissions(read: true, propose: true);
    await session.handle(call('read', 'github_read_file', {}));
    final repeated = await session.handle(call('read', 'github_read_file', {}));
    expect(repeated.toMap().toString(), contains('DUPLICATE'));
    expect(reads, 1);
    final duplicateKeys = OpenAIResponseToolCall.fromMap({
      'id': 'proposal', 'type': 'function', 'function': {
        'name': 'github_propose_replacement',
        'arguments': '{"original_sha":"original-sha","replacement":"new","replacement":"hidden"}'}});
    final result = await session.handle(duplicateKeys);
    expect(result.toMap().toString(), contains('INVALID_ARGUMENTS'));
    expect(session.pending, isNull);
  });


  test('pending proposal is immutable across later tool calls', () async {
    final session = GitHubToolSession(
      repository: 'owner/repo', branch: 'work/test', path: 'a.txt',
      read: () async => snapshot,
      commit: (_, __) async => throw StateError('no approval given'),
    );
    session.setPermissions(read: true, propose: true);
    await session.handle(call('read1', 'github_read_file', {}));
    await session.handle(call('propose1', 'github_propose_replacement',
      {'original_sha': 'original-sha', 'replacement': 'first'}));
    final originalId = session.pending!.id;
    final attempt = await session.handle(call('propose2',
      'github_propose_replacement', {
        'original_sha': 'original-sha', 'replacement': 'second',
      }));
    final reread = await session.handle(call('read2', 'github_read_file', {}));
    expect(attempt.toMap().toString(), contains('APPROVAL_PENDING'));
    expect(reread.toMap().toString(), contains('APPROVAL_PENDING'));
    expect(session.pending!.id, originalId);
    expect(session.pending!.replacement, 'first');
  });

}

