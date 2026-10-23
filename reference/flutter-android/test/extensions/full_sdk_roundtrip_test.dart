import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:github/github.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/conversation_controller.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';

const originalBlob = 'cccccccccccccccccccccccccccccccccccccccc';
const committedBlob = 'dddddddddddddddddddddddddddddddddddddddd';
const commitId = 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee';
const originalText = 'original\n';
const approvedText = 'approved\n';

class FixtureConfig extends ConnectorConfigStore {
  @override
  Future<ConnectorConfig> load() async => const ConnectorConfig(
    apiKey: 'fake-model-key', modelId: 'fixture-model',
    githubToken: 'fake-github-token', repository: 'owner/repo',
    branch: 'work/test', historyScope: 'abcdabcdabcdabcdabcdabcdabcdabcd',
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  test('actual AI + GitHub SDK adapters complete read/proposal/approved write/commit verification', () async {
    // Flutter's widget-test HTTP override returns 400 to all real HTTP clients.
    // Only this isolated loopback fixture is allowed to use an actual HttpClient.
    final originalOverrides = HttpOverrides.current;
    HttpOverrides.global = null;
    addTearDown(() => HttpOverrides.global = originalOverrides);
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var gets = 0, puts = 0, aiCalls = 0;
    var toolText = '', sentMessages = '';
    server.listen((request) async {
      request.response.headers.contentType = ContentType.json;
      final path = request.uri.path;
      if (request.method == 'GET' && path == '/repos/owner/repo') {
        gets++;
        request.response.write(jsonEncode({'name': 'repo', 'default_branch': 'main'}));
      } else if (request.method == 'GET' && path.endsWith('/contents/file.txt')) {
        gets++;
        final pinned = request.uri.queryParameters['ref'] == commitId;
        final text = pinned ? approvedText : originalText;
        request.response.write(jsonEncode({
          'type': 'file', 'name': 'file.txt', 'path': 'file.txt',
          'sha': pinned ? committedBlob : originalBlob,
          'encoding': 'base64', 'size': utf8.encode(text).length,
          'content': base64Encode(utf8.encode(text)),
        }));
      } else if (request.method == 'PUT' && path.endsWith('/contents/file.txt')) {
        puts++;
        final payload = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
        expect(payload['sha'], originalBlob);
        expect(payload['branch'], 'work/test');
        expect(utf8.decode(base64Decode(payload['content'] as String)), approvedText);
        request.response.write(jsonEncode({
          'content': {'sha': committedBlob, 'path': 'file.txt', 'type': 'file'},
          'commit': {'sha': commitId},
        }));
      } else {
        request.response.statusCode = HttpStatus.notFound;
        request.response.write(jsonEncode({'message': 'unexpected fixture request'}));
      }
      await request.response.close();
    });

    final repo = AndroidGitHubExtension(
      createClient: (token) => GitHub(
        endpoint: 'http://127.0.0.1:${server.port}',
        auth: Authentication.withToken(token),
      ),
    );
    final host = AndroidConversationController(
      store: FixtureConfig(), github: repo,
      requestClientFactory: () => MockClient((request) async {
        aiCalls++;
        expect(request.headers['Authorization'], 'Bearer fake-model-key');
        final body = jsonDecode(request.body) as Map<String, dynamic>;
        expect(body['model'], 'fixture-model');
        final sent = body['messages'] as List;
        if (aiCalls == 1) {
          expect(sent.last['role'], 'user');
        } else {
          expect(aiCalls, 2);
          sentMessages = jsonEncode(sent);
          final result = sent.where((m) => m['role'] == 'tool').toList();
          expect(result, hasLength(1));
          expect(result.first['tool_call_id'], 'read');
          toolText = result.first['content'].toString();
        }
        final name = aiCalls == 1
            ? 'github_read_file' : 'github_propose_replacement';
        final args = aiCalls == 1 ? <String, String>{} : <String, String>{
          'original_sha': originalBlob, 'replacement': approvedText,
        };
        return http.Response(jsonEncode({
          'id': 'ai-$aiCalls', 'object': 'chat.completion',
          'created': 1, 'model': 'fixture-model',
          'choices': [{
            'index': 0, 'finish_reason': 'tool_calls',
            'message': {'role': 'assistant', 'content': null, 'tool_calls': [{
              'id': aiCalls == 1 ? 'read' : 'propose',
              'type': 'function',
              'function': {'name': name, 'arguments': jsonEncode(args)},
            }]},
          }],
          'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2},
        }), 200);
      }),
    );
    addTearDown(host.dispose);
    await host.load();
    host.selectFile('file.txt', read: true, propose: true);
    await host.send('edit the selected file');
    expect(host.githubSession?.pending, isNotNull,
        reason: 'status=${host.status}; calls=$aiCalls; tool=$toolText; messages=$sentMessages; gets=$gets');
    expect(aiCalls, 2);
    expect(puts, 0);
    final pending = host.githubSession!.pending!;
    expect(pending.snapshot.blobSha, originalBlob);
    expect(pending.replacement, approvedText);
    await host.approve(pending.id);
    expect(puts, 1);
    expect(gets, 2); // selected file and exact pinned commit
    expect(host.status, contains(commitId));
    await host.approve(pending.id);
    expect(puts, 1); // consumed approval must never replay
  });
}
