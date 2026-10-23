import 'dart:convert';
import 'package:dart_openai/dart_openai.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:wex/extensions/android_connectors/ai_extension.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';

void main() {
  test(
    'SDK sends one version segment and preserves native tool calls',
    () async {
      var requests = 0;
      final client = MockClient((request) async {
        requests++;
        expect(
          request.url.toString(),
          'https://example.com/custom/v1/chat/completions',
        );
        expect(request.headers['Authorization'], 'Bearer secret');
        final body = jsonDecode(request.body) as Map;
        expect(body['model'], 'chosen-model');
        return http.Response(
          jsonEncode({
            'id': 'chat-1',
            'object': 'chat.completion',
            'created': 1,
            'model': 'chosen-model',
            'choices': [
              {
                'index': 0,
                'finish_reason': 'tool_calls',
                'message': {
                  'role': 'assistant',
                  'content': null,
                  'tool_calls': [
                    {
                      'id': 'read1',
                      'type': 'function',
                      'function': {
                        'name': 'github_read_file',
                        'arguments': '{}',
                      },
                    },
                  ],
                },
              },
            ],
            'usage': {
              'prompt_tokens': 1,
              'completion_tokens': 1,
              'total_tokens': 2,
            },
          }),
          200,
        );
      });
      final message = await AndroidAiExtension().complete(
        const ConnectorConfig(
          apiBaseUrl: 'https://example.com/custom/v1/',
          apiKey: 'secret',
          modelId: 'chosen-model',
        ),
        [
          OpenAIChatCompletionChoiceMessageModel(
            role: OpenAIChatMessageRole.user,
            content: [
              OpenAIChatCompletionChoiceMessageContentItemModel.text('hello'),
            ],
          ),
        ],
        const [],
        client,
      );
      expect(message.toolCalls!.single.id, 'read1');
      expect(requests, 1);
    },
  );

  test('SDK does not silently retry failed billable requests', () async {
    var requests = 0;
    final client = MockClient((request) async {
      requests++;
      expect(request.followRedirects, isFalse);
      return http.Response(
        '{"error":{"message":"temporary","type":"error"}}',
        503,
      );
    });
    await expectLater(
      AndroidAiExtension().complete(
        const ConnectorConfig(apiKey: 'secret', modelId: 'chosen-model'),
        [],
        [],
        client,
      ),
      throwsA(anything),
    );
    expect(requests, 1);
  });
}
