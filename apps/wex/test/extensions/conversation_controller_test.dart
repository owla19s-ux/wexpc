import 'dart:async';
import 'dart:convert';
import 'package:dart_openai/dart_openai.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:wex/extensions/android_connectors/ai_extension.dart';
import 'package:wex/extensions/android_connectors/browser_extension.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/conversation_controller.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';

const verifiedCommit = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const verifiedBlob = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const config = ConnectorConfig(
  historyScope: '123456789abcdef0123456789abcdef0',
  apiKey: 'not-a-real-key',
  modelId: 'fixture-model',
  repository: 'owner/repo',
  branch: 'work/test',
  githubToken: 'fixture-token',
);

class Store extends ConnectorConfigStore {
  @override
  Future<ConnectorConfig> load() async => config;
}

class Ai extends AndroidAiExtension {
  Ai(this.respond);
  final Future<OpenAIChatCompletionChoiceMessageModel> Function(
    List<OpenAIToolModel>,
  )
  respond;
  @override
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools,
    http.Client client,
  ) => respond(tools);
}

class Repo extends AndroidGitHubExtension {
  int writes = 0;
  @override
  Future<RepoFileSnapshot> read(ConnectorConfig config, String path) async =>
      const RepoFileSnapshot(
        repository: 'owner/repo',
        branch: 'work/test',
        path: 'file.txt',
        blobSha: 'blob-before',
        text: 'before',
      );
  @override
  Future<GitHubWriteReceipt> commitApprovedReplacement(
    ConnectorConfig config,
    RepoFileSnapshot snapshot,
    String replacement,
  ) async {
    writes++;
    expect(replacement, 'after');
    return const GitHubWriteReceipt(
      commitSha: verifiedCommit,
      newBlobSha: verifiedBlob,
      verified: true,
    );
  }
}


class UnverifiedRepo extends Repo {
  @override
  Future<GitHubWriteReceipt> commitApprovedReplacement(
    ConnectorConfig config, RepoFileSnapshot snapshot, String replacement,
  ) async {
    writes++;
    return const GitHubWriteReceipt(
      commitSha: verifiedCommit, newBlobSha: verifiedBlob, verified: false,
    );
  }
}

OpenAIChatCompletionChoiceMessageModel tool(
  String id,
  String name,
  Map<String, Object> args,
) => OpenAIChatCompletionChoiceMessageModel(
  role: OpenAIChatMessageRole.assistant,
  content: null,
  toolCalls: [
    OpenAIResponseToolCall.fromMap({
      'id': id,
      'type': 'function',
      'function': {'name': name, 'arguments': jsonEncode(args)},
    }),
  ],
);


class InspectAi extends AndroidAiExtension {
  String lastSystem = '';
  List<OpenAIToolModel> lastTools = [];
  @override
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools,
    http.Client client,
  ) async {
    lastSystem = (messages.first.content ?? []).map((m) => m.text ?? '').join();
    lastTools = tools;
    return OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('ok')],
    );
  }
}

class DelayedCommitRepo extends Repo {
  final started = Completer<void>();
  final result = Completer<GitHubWriteReceipt>();
  @override
  Future<GitHubWriteReceipt> commitApprovedReplacement(
    ConnectorConfig config, RepoFileSnapshot snapshot, String replacement,
  ) {
    writes++;
    started.complete();
    return result.future;
  }
}

class DelayedReadRepo extends Repo {
  final started = Completer<void>();
  final result = Completer<RepoFileSnapshot>();
  @override
  Future<RepoFileSnapshot> read(ConnectorConfig config, String path) {
    started.complete();
    return result.future;
  }
}

class ReadReceiptAi extends AndroidAiExtension {
  int calls = 0;
  String lastTool = '';
  @override
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools, http.Client client,
  ) async {
    if (calls++ == 0) return tool('delayed-file', 'github_read_file', {});
    lastTool = (messages.last.content ?? []).map((m) => m.text ?? '').join();
    return OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('done')],
    );
  }
}

class ControlledPage extends AndroidBrowserExtension {
  final started = Completer<void>();
  final result = Completer<Map<String, Object?>>();
  bool cancelled = false;
  @override
  Future<Map<String, Object?>> read(String url) {
    started.complete();
    return result.future;
  }
  @override
  void cancel() { cancelled = true; }
}

class BrowserAi extends AndroidAiExtension {
  int calls = 0;
  String lastTool = '';
  @override
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools, http.Client client,
  ) async {
    if (calls++ == 0) return tool('public-read', 'browser_read_public', {});
    lastTool = (messages.last.content ?? []).map((m) => m.text ?? '').join();
    return OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('done')],
    );
  }
}

class DelayedStore extends ConnectorConfigStore {
  final started = Completer<void>();
  final release = Completer<void>();
  bool delayed = true;
  @override
  Future<ConnectorConfig> load() async {
    final value = await super.load();
    if (delayed) {
      delayed = false;
      started.complete();
      await release.future;
    }
    return value;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));



  test('GitHub disable does not retry or discard an in-flight approved commit', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final repo = DelayedCommitRepo();
    var rounds = 0;
    final host = AndroidConversationController(
      store: store, github: repo,
      ai: Ai((_) async => rounds++ == 0
        ? tool('read-commit', 'github_read_file', {})
        : tool('propose-commit', 'github_propose_replacement', {
            'original_sha': 'blob-before', 'replacement': 'after',
          })),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://example.com/', true);
    await host.send('replace');
    final id = host.githubSession!.pending!.id;
    final committing = host.approve(id);
    await repo.started.future;
    await store.save((await store.load()).withAvailability(githubAvailable: false));
    expect(host.busy, isTrue);
    expect(host.committing, isTrue);
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isTrue);
    await host.approve(id);
    expect(repo.writes, 1);
    repo.result.complete(const GitHubWriteReceipt(
      commitSha: verifiedCommit, newBlobSha: verifiedBlob, verified: true,
    ));
    await committing;
    expect(host.status, contains(verifiedCommit));
    expect(host.busy, isFalse);
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isTrue);
  });

  test('config deletion and unknown revision still revoke every module', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final host = AndroidConversationController(store: store);
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://example.com/', true);
    // Missing metadata must never be treated as an availability-only change.
    ConnectorConfigStore.revisions.value++;
    expect(host.config, isNull);
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isFalse);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://example.com/', true);
    await store.delete();
    expect(host.config, isNull);
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isFalse);
  });

  test('browser availability changes preserve GitHub proposal and AI identity', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    var rounds = 0;
    final host = AndroidConversationController(
      store: store, github: Repo(),
      ai: Ai((_) async => rounds++ == 0
        ? tool('read-independent', 'github_read_file', {})
        : tool('propose-independent', 'github_propose_replacement', {
            'original_sha': 'blob-before', 'replacement': 'after',
          })),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://example.com/', true);
    await host.send('replace');
    final proposal = host.githubSession!.pending;
    final session = host.githubSession;
    final scope = host.config!.historyScope;
    final count = host.entries.length;
    await store.save((await store.load()).withAvailability(browserAvailable: false));
    expect(host.config!.hasAi, isTrue);
    expect(host.config!.historyScope, scope);
    expect(host.entries, hasLength(count));
    expect(host.browserEnabled, isFalse);
    expect(host.canRead, isTrue);
    expect(host.canPropose, isTrue);
    expect(host.githubSession, same(session));
    expect(host.githubSession!.pending, same(proposal));
    await store.save((await store.load()).withAvailability(browserAvailable: true));
    expect(host.browserEnabled, isFalse);
    expect(host.githubSession!.pending, same(proposal));
  });

  test('GitHub availability revokes only GitHub and reenable does not grant', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final host = AndroidConversationController(store: store, ai: InspectAi());
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://example.com/', true);
    await store.save((await store.load()).withAvailability(githubAvailable: false));
    expect(host.config!.hasAi, isTrue);
    expect(host.githubSession, isNull);
    expect(host.canRead, isFalse);
    expect(host.canPropose, isFalse);
    expect(host.browserEnabled, isTrue);
    expect(() => host.selectFile('file.txt', read: true, propose: true), throwsStateError);
    await store.save((await store.load()).withAvailability(githubAvailable: true));
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isTrue);
  });

  test('disabling AI stops late output while preserving unrelated grants', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final started = Completer<void>();
    final delayed = Completer<OpenAIChatCompletionChoiceMessageModel>();
    final host = AndroidConversationController(
      store: store, ai: Ai((_) { started.complete(); return delayed.future; }),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://example.com/', true);
    final session = host.githubSession;
    final sent = host.send('hello');
    await started.future;
    await store.save((await store.load()).withAvailability(aiAvailable: false));
    expect(host.config!.hasAi, isFalse);
    expect(host.busy, isFalse);
    expect(host.githubSession, same(session));
    expect(host.canRead, isTrue);
    expect(host.browserEnabled, isTrue);
    delayed.complete(OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('late-disabled')],
    ));
    await sent;
    expect(host.entries.map((e) => e.text), isNot(contains('late-disabled')));
    await store.save((await store.load()).withAvailability(aiAvailable: true));
    expect(host.config!.hasAi, isTrue);
    expect(host.entries.map((e) => e.text), ['hello']);
  });

  test('saving identical config does not interrupt an active request or grants', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final started = Completer<void>();
    final delayed = Completer<OpenAIChatCompletionChoiceMessageModel>();
    final host = AndroidConversationController(
      store: store, ai: Ai((_) { started.complete(); return delayed.future; }),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: false);
    host.selectBrowser('https://example.com/', true);
    final sent = host.send('hello');
    await started.future;
    await store.save(await store.load());
    expect(host.busy, isTrue);
    expect(host.canRead, isTrue);
    expect(host.browserEnabled, isTrue);
    delayed.complete(OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('accepted-answer')],
    ));
    await sent;
    expect(host.entries.last.text, 'accepted-answer');
  });

  test('revoked GitHub read never discloses late file content to AI', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final repo = DelayedReadRepo();
    final ai = ReadReceiptAi();
    final host = AndroidConversationController(store: store, github: repo, ai: ai);
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: false);
    host.selectBrowser('https://example.com/', true);
    final sent = host.send('read file');
    await repo.started.future;
    await store.save((await store.load()).withAvailability(githubAvailable: false));
    repo.result.complete(const RepoFileSnapshot(
      repository: 'owner/repo', branch: 'work/test', path: 'file.txt',
      blobSha: 'late-sha', text: 'late-private-content',
    ));
    await sent;
    expect(ai.calls, 2);
    expect(ai.lastTool, contains('DENIED'));
    expect(ai.lastTool, isNot(contains('late-private-content')));
    expect(host.browserEnabled, isTrue);
    expect(host.canRead, isFalse);
  });

  test('revoked browser result is denied before the next AI request', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final page = ControlledPage();
    final ai = BrowserAi();
    final host = AndroidConversationController(
      store: store, ai: ai, browserFactory: () => page,
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: false);
    host.selectBrowser('https://example.com/', true);
    final sent = host.send('read page');
    await page.started.future;
    await store.save((await store.load()).withAvailability(browserAvailable: false));
    expect(page.cancelled, isTrue);
    page.result.complete({'text': 'late-secret-page'});
    await sent;
    expect(ai.calls, 2);
    expect(ai.lastTool, contains('DENIED'));
    expect(ai.lastTool, isNot(contains('late-secret-page')));
    expect(host.canRead, isTrue);
    expect(host.config!.hasAi, isTrue);
  });

  test('availability save invalidates an older load instead of restoring stale flags', () async {
    final store = ConnectorConfigStore();
    await store.save(config);
    final delayed = DelayedStore();
    final host = AndroidConversationController(store: delayed);
    addTearDown(host.dispose);
    final loading = host.load();
    await delayed.started.future;
    await store.save((await store.load()).withAvailability(browserAvailable: false));
    delayed.release.complete();
    await loading;
    expect(host.config, isNull);
    await host.load();
    expect(host.config!.browserAvailable, isFalse);
  });

  test('submission acceptance is synchronous and rejects duplicate or busy sends', () async {
    final delayed = Completer<OpenAIChatCompletionChoiceMessageModel>();
    final started = Completer<void>();
    final host = AndroidConversationController(store: Store(), ai: Ai((_) {
      started.complete();
      return delayed.future;
    }));
    addTearDown(host.dispose);
    expect(host.submit('not configured').accepted, isFalse);
    await host.load();
    expect(host.submit('   ').accepted, isFalse);
    expect(host.submit(List.filled(12001, '文').join()).accepted, isFalse);
    expect(host.entries, isEmpty);
    final first = host.submit('hello');
    expect(first.accepted, isTrue);
    expect(host.busy, isTrue);
    expect(host.entries.map((e) => e.text), ['hello']);
    expect(host.submit('duplicate').accepted, isFalse);
    await started.future;
    delayed.complete(OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('answer')],
    ));
    await first.completed;
    expect(host.entries.map((e) => e.text), ['hello', 'answer']);
    expect(host.busy, isFalse);
  });

  test('submission with a pending proposal is rejected without consuming it', () async {
    var rounds = 0;
    final host = AndroidConversationController(
      store: Store(), github: Repo(),
      ai: Ai((_) async => rounds++ == 0
        ? tool('read-pending', 'github_read_file', {})
        : tool('propose-pending', 'github_propose_replacement', {
            'original_sha': 'blob-before', 'replacement': 'after',
          })),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    await host.send('replace');
    final proposal = host.githubSession!.pending;
    final count = host.entries.length;
    final result = host.submit('another message');
    expect(result.accepted, isFalse);
    await result.completed;
    expect(host.githubSession!.pending, same(proposal));
    expect(host.entries, hasLength(count));
    expect(host.status, contains('批准或丢弃'));
  });

  test('client creation failure completes without leaving the host busy', () async {
    final host = AndroidConversationController(
      store: Store(),
      requestClientFactory: () => throw StateError('fixture client failure'),
    );
    addTearDown(host.dispose);
    await host.load();
    final result = host.submit('accepted locally');
    expect(result.accepted, isTrue);
    await result.completed;
    expect(host.busy, isFalse);
    expect(host.entries.single.text, 'accepted locally');
    expect(host.status, contains('请求失败'));
  });

  test('disposed host rejects submission without creating a request', () async {
    final host = AndroidConversationController(store: Store(), ai: InspectAi());
    await host.load();
    host.dispose();
    final result = host.submit('late');
    expect(result.accepted, isFalse);
    await result.completed;
    expect(host.entries, isEmpty);
  });

  test('revoked tools do not leak selected resource names to the AI', () async {
    final ai = InspectAi();
    final host = AndroidConversationController(store: Store(), ai: ai);
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: false);
    host.selectBrowser('https://docs.flutter.dev/', true);
    host.selectFile('file.txt', read: false, propose: false);
    host.selectBrowser('', false);
    await host.send('ordinary conversation');
    expect(ai.lastSystem, isNot(contains('owner/repo')));
    expect(ai.lastSystem, isNot(contains('file.txt')));
    expect(ai.lastSystem, isNot(contains('docs.flutter.dev')));
    expect(ai.lastTools, isEmpty);

    host.selectFile('file.txt', read: true, propose: false);
    host.selectBrowser('https://docs.flutter.dev/', true);
    await host.send('use authorized resources');
    expect(ai.lastSystem, contains('owner/repo'));
    expect(ai.lastSystem, contains('file.txt'));
    expect(ai.lastSystem, contains('docs.flutter.dev'));
    expect(ai.lastTools, hasLength(2));
  });

  test(
    'conversation loads closed tools and requires human approval for commit',
    () async {
      var rounds = 0;
      final repo = Repo();
      final host = AndroidConversationController(
        store: Store(),
        github: repo,
        ai: Ai((tools) async {
          expect(tools.length, 2);
          return rounds++ == 0
              ? tool('read', 'github_read_file', {})
              : tool('proposal', 'github_propose_replacement', {
                  'original_sha': 'blob-before',
                  'replacement': 'after',
                });
        }),
      );
      addTearDown(host.dispose);
      await host.load();
      expect(host.canRead, isFalse);
      expect(host.browserEnabled, isFalse);
      host.selectFile('file.txt', read: true, propose: true);
      await host.send('change selected file');
      expect(repo.writes, 0);
      expect(host.githubSession!.pending!.replacement, 'after');
      final id = host.githubSession!.pending!.id;
      await host.approve(id);
      expect(repo.writes, 1);
      expect(host.status, contains(verifiedCommit));
      await host.approve(id);
      expect(repo.writes, 1);
    },
  );

  test('stop rejects late model output and revokes all tools', () async {
    final delayed = Completer<OpenAIChatCompletionChoiceMessageModel>();
    final started = Completer<void>();
    final host = AndroidConversationController(
      store: Store(),
      ai: Ai((_) {
        started.complete();
        return delayed.future;
      }),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectBrowser('https://docs.flutter.dev/', true);
    final pending = host.send('hello');
    await started.future;
    host.stop();
    delayed.complete(
      OpenAIChatCompletionChoiceMessageModel(
        role: OpenAIChatMessageRole.assistant,
        content: [
          OpenAIChatCompletionChoiceMessageContentItemModel.text(
            'late response',
          ),
        ],
      ),
    );
    await pending;
    expect(host.entries.map((x) => x.text), isNot(contains('late response')));
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isFalse);
    expect(host.busy, isFalse);
  });

  test('autosaved text restores without restoring tool permissions', () async {
    final host = AndroidConversationController(
      store: Store(),
      ai: Ai(
        (_) async => OpenAIChatCompletionChoiceMessageModel(
          role: OpenAIChatMessageRole.assistant,
          content: [
            OpenAIChatCompletionChoiceMessageContentItemModel.text('answer'),
          ],
        ),
      ),
    );
    await host.load();
    await host.send('hello');
    host.dispose();
    final reopened = AndroidConversationController(store: Store());
    addTearDown(reopened.dispose);
    await reopened.load();
    expect(reopened.entries.map((x) => x.text).toList(), ['hello', 'answer']);
    expect(reopened.canRead, isFalse);
    expect(reopened.githubSession, isNull);
  });

  test('full SDK chat -> read -> proposal -> human approval -> receipt', () async {
    final repo = Repo();
    var requests = 0;
    final host = AndroidConversationController(
      store: Store(), github: repo,
      requestClientFactory: () => MockClient((request) async {
        requests++;
        expect(request.url.path, '/v1/chat/completions');
        final body = jsonDecode(request.body) as Map<String, dynamic>;
        expect(body['model'], 'fixture-model');
        if (requests == 1) {
          expect(body['tools'], hasLength(2));
          expect((body['messages'] as List).last['role'], 'user');
        } else {
          expect(requests, 2);
          final messages = body['messages'] as List;
          final toolMessages = messages.where((m) => m['role'] == 'tool').toList();
          expect(toolMessages, hasLength(1));
          expect(toolMessages.first['tool_call_id'], 'call-read');
          expect(toolMessages.first['content'].toString(), contains('before'));
        }
        final function = requests == 1 ? 'github_read_file' : 'github_propose_replacement';
        final args = requests == 1 ? <String, Object>{} : <String, Object>{
          'original_sha': 'blob-before', 'replacement': 'after',
        };
        return http.Response(jsonEncode({
          'id': 'sdk-$requests', 'object': 'chat.completion',
          'created': 1, 'model': 'fixture-model',
          'choices': [{
            'index': 0, 'finish_reason': 'tool_calls',
            'message': {'role': 'assistant', 'content': null, 'tool_calls': [{
              'id': 'call-${requests == 1 ? "read" : "propose"}',
              'type': 'function', 'function': {
                'name': function, 'arguments': jsonEncode(args),
              },
            }]},
          }],
          'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2},
        }), 200);
      }),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    await host.send('update file');
    expect(requests, 2);
    expect(repo.writes, 0);
    final proposal = host.githubSession!.pending!;
    expect(proposal.replacement, 'after');
    await host.approve(proposal.id);
    expect(repo.writes, 1);
    expect(host.status, contains(verifiedCommit));
  });

  test('saving changed API connection revokes old tools and isolates history', () async {
    final store = ConnectorConfigStore();
    await store.save(const ConnectorConfig(
      apiKey: 'account-a', modelId: 'model-a',
      githubToken: 'github-a', repository: 'owner/repo', branch: 'work/test',
    ));
    final initialScope = (await store.load()).historyScope;
    final host = AndroidConversationController(store: store, ai: InspectAi());
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    await host.send('private-to-a');
    expect(host.entries, hasLength(2));
    await store.save(const ConnectorConfig(
      apiKey: 'account-b', modelId: 'model-a',
      githubToken: 'github-a', repository: 'owner/repo', branch: 'work/test',
    ));
    expect(host.config, isNull);
    expect(host.canRead, isFalse);
    expect(host.githubSession, isNull);
    final nextScope = (await store.load()).historyScope;
    expect(nextScope, isNot(initialScope));
    await host.load();
    expect(host.entries, isEmpty);
  });

  test('changing GitHub target also isolates historic resource content', () async {
    final store = ConnectorConfigStore();
    await store.save(const ConnectorConfig(
      apiKey: 'unchanged-ai', modelId: 'unchanged-model',
      githubToken: 'one', repository: 'owner/repo', branch: 'work/one',
    ));
    final initial = (await store.load()).historyScope;
    final host = AndroidConversationController(store: store, ai: InspectAi());
    addTearDown(host.dispose);
    await host.load();
    await host.send('private repository A details');
    await store.save(const ConnectorConfig(
      apiKey: 'unchanged-ai', modelId: 'unchanged-model',
      githubToken: 'one', repository: 'owner/repo', branch: 'work/two',
    ));
    expect(host.config, isNull);
    final next = (await store.load()).historyScope;
    expect(next, isNot(initial));
    await host.load();
    expect(host.entries, isEmpty);
  });

  test('GitHub read can be revoked with an empty invalid path field', () async {
    final host = AndroidConversationController(store: Store());
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    host.selectFile('', read: false, propose: false);
    expect(host.canRead, isFalse);
    expect(host.githubSession, isNull);
  });

  test('rejects adapter receipt without verification', () async {
    final repo = UnverifiedRepo();
    var rounds = 0;
    final host = AndroidConversationController(
      store: Store(), github: repo,
      ai: Ai((_) async => rounds++ == 0
        ? tool('read-unverified', 'github_read_file', {})
        : tool('propose-unverified', 'github_propose_replacement', {
            'original_sha': 'blob-before', 'replacement': 'after',
          })),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    await host.send('replace');
    final id = host.githubSession!.pending!.id;
    await host.approve(id);
    expect(repo.writes, 1);
    expect(host.status, contains('结果未知'));
    expect(host.entries.any((e) => e.text.contains('真实执行回执')), isFalse);
  });

}
