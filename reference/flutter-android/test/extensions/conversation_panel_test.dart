import 'dart:convert';
import 'package:dart_openai/dart_openai.dart';
import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:wex/extensions/android_connectors/ai_extension.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/conversation_controller.dart';
import 'package:wex/extensions/android_connectors/conversation_panel.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';

const _config = ConnectorConfig(
  apiKey: 'fixture', modelId: 'fixture-model',
  githubToken: 'fixture', repository: 'owner/repo', branch: 'work/test',
  historyScope: '123456789abcdef0123456789abcdef0',
);

class _Store extends ConnectorConfigStore {
  @override
  Future<ConnectorConfig> load() async => _config;
}

class _Ai extends AndroidAiExtension {
  _Ai({this.propose = false});
  final bool propose;
  int requests = 0;
  @override
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools,
    http.Client client,
  ) async {
    requests++;
    if (!propose) {
      return OpenAIChatCompletionChoiceMessageModel(
        role: OpenAIChatMessageRole.assistant,
        content: [OpenAIChatCompletionChoiceMessageContentItemModel.text('fixture answer')],
      );
    }
    return OpenAIChatCompletionChoiceMessageModel(
      role: OpenAIChatMessageRole.assistant,
      content: null,
      toolCalls: [OpenAIResponseToolCall.fromMap({
        'id': 'call-$requests', 'type': 'function',
        'function': {
          'name': requests == 1 ? 'github_read_file' : 'github_propose_replacement',
          'arguments': jsonEncode(requests == 1 ? <String, Object>{} : {
            'original_sha': 'blob-before', 'replacement': 'after',
          }),
        },
      })],
    );
  }
}

class _Repo extends AndroidGitHubExtension {
  int writes = 0;
  @override
  Future<RepoFileSnapshot> read(ConnectorConfig config, String path) async =>
      const RepoFileSnapshot(
        repository: 'owner/repo', branch: 'work/test', path: 'file.txt',
        blobSha: 'blob-before', text: 'before',
      );
  @override
  Future<GitHubWriteReceipt> commitApprovedReplacement(
    ConnectorConfig config, RepoFileSnapshot snapshot, String replacement,
  ) async {
    expect(snapshot.repository, 'owner/repo');
    expect(snapshot.branch, 'work/test');
    expect(snapshot.path, 'file.txt');
    expect(snapshot.blobSha, 'blob-before');
    expect(replacement, 'after');
    writes++;
    return const GitHubWriteReceipt(
      commitSha: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      newBlobSha: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', verified: true,
    );
  }
}

void main() {
  testWidgets('stale send callback cannot clear the next draft while busy', (tester) async {
    final ai = _Ai();
    final host = AndroidConversationController(store: _Store(), ai: ai);
    final draft = TextEditingController(text: 'first');
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    await host.load();
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(body: AndroidConversationPanel(controller: host, draft: draft)),
    ));
    final callback = tester.widget<IconButton>(
      find.byKey(const ValueKey('send-message')),
    ).onPressed!;
    callback();
    expect(draft.text, isEmpty);
    draft.text = 'next draft';
    callback();
    expect(draft.text, 'next draft');
    await tester.pumpAndSettle();
    expect(ai.requests, 1);
    expect(draft.text, 'next draft');
    expect(host.entries.where((e) => e.role == 'user').map((e) => e.text), ['first']);
  });

  testWidgets('oversized rejected submission keeps the complete draft', (tester) async {
    final ai = _Ai();
    final host = AndroidConversationController(store: _Store(), ai: ai);
    final text = List.filled(12001, '文').join();
    final draft = TextEditingController(text: text);
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    await host.load();
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(body: AndroidConversationPanel(controller: host, draft: draft)),
    ));
    await tester.tap(find.byKey(const ValueKey('send-message')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('composer-state')), findsOneWidget);
    expect(find.text('单条消息限 12000 字符'), findsWidgets);
    expect(draft.text, text);
    expect(host.entries, isEmpty);
    expect(ai.requests, 0);
    expect(host.status, contains('12000'));
  });

  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  Future<void> showPanel(
    WidgetTester tester, AndroidConversationController host, TextEditingController draft,
  ) => tester.pumpWidget(MaterialApp(
    home: Scaffold(body: AndroidConversationPanel(controller: host, draft: draft)),
  ));

  testWidgets('ordinary chat uses supplied host and draft without tools', (tester) async {
    final ai = _Ai();
    final host = AndroidConversationController(store: _Store(), ai: ai);
    final draft = TextEditingController();
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    await host.load();
    await showPanel(tester, host, draft);
    await tester.enterText(find.byKey(const ValueKey('message-draft')), 'hello');
    await tester.tap(find.byKey(const ValueKey('send-message')));
    await tester.pumpAndSettle();
    expect(ai.requests, 1);
    expect(host.entries.map((e) => e.text), ['hello', 'fixture answer']);
    expect(draft.text, isEmpty);
    expect(host.canRead, isFalse);
    expect(host.browserEnabled, isFalse);
    expect(find.text('fixture answer'), findsOneWidget);

    await tester.pumpWidget(const SizedBox.shrink());
    await host.load();
    expect(host.config!.modelId, 'fixture-model');
  });

  testWidgets('tool panel toggles only explicit permissions and preserves draft', (tester) async {
    final host = AndroidConversationController(store: _Store());
    final draft = TextEditingController(text: 'keep draft');
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    await host.load();
    await showPanel(tester, host, draft);
    await tester.tap(find.text('本对话工具授权'));
    await tester.pumpAndSettle();
    final path = find.widgetWithText(TextField, '仓库内已有文件路径，如 README.md');
    await tester.enterText(path, 'file.txt');
    await tester.ensureVisible(find.text('允许 AI 读取所选文件'));
    await tester.tap(find.text('允许 AI 读取所选文件'));
    await tester.pumpAndSettle();
    expect(host.canRead, isTrue);
    expect(host.canPropose, isFalse);
    expect(host.browserEnabled, isFalse);
    await tester.enterText(path, '');
    await tester.tap(find.text('允许 AI 读取所选文件'));
    await tester.pumpAndSettle();
    expect(host.canRead, isFalse);
    expect(host.githubSession, isNull);
    expect(draft.text, 'keep draft');
    await tester.tap(find.text('本对话工具授权'));
    await tester.pumpAndSettle();
    expect(draft.text, 'keep draft');
  });

  testWidgets('diff dismissal never commits and approval uses exact proposal once', (tester) async {
    final repo = _Repo();
    final host = AndroidConversationController(store: _Store(), ai: _Ai(propose: true), github: repo);
    final draft = TextEditingController();
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    await host.send('replace');
    await showPanel(tester, host, draft);
    expect(repo.writes, 0);
    draft.text = 'keep while waiting for approval';
    await tester.tap(find.byKey(const ValueKey('send-message')));
    await tester.pumpAndSettle();
    expect(draft.text, 'keep while waiting for approval');
    expect(host.status, contains('批准或丢弃'));
    expect(find.text('有待审批的 GitHub 修改；请先批准或丢弃'), findsOneWidget);
    await tester.tap(find.text('查看差异并审批'));
    await tester.pumpAndSettle();
    expect(find.textContaining('原 Blob：blob-before'), findsOneWidget);
    expect(find.textContaining('- before'), findsOneWidget);
    expect(find.textContaining('+ after'), findsOneWidget);
    await tester.tap(find.text('暂不提交'));
    await tester.pumpAndSettle();
    expect(repo.writes, 0);
    expect(host.githubSession!.pending, isNotNull);
    await tester.tap(find.text('查看差异并审批'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('批准并提交'));
    await tester.pumpAndSettle();
    expect(repo.writes, 1);
    expect(host.githubSession!.pending, isNull);
    expect(find.text('查看差异并审批'), findsNothing);
    expect(host.status, contains('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'));
  });
}
