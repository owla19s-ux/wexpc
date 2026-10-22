import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:wex/extensions/android_connectors/ai_extension.dart';
import 'package:wex/extensions/android_connectors/browser_extension.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/connector_diagnostics.dart';
import 'package:dart_openai/dart_openai.dart';

class TrackedClient extends http.BaseClient {
  bool closed = false;
  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) =>
      throw StateError('No network in fixture');
  @override
  void close() { closed = true; }
}

class FailingAi extends AndroidAiExtension {
  @override
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools,
    http.Client client,
  ) async {
    expect(tools, isEmpty);
    expect(messages, hasLength(1));
    throw StateError('fixture failure');
  }
}

class Page extends AndroidBrowserExtension {
  Page(this.result);
  final Map<String, Object?> result;
  bool stopped = false;
  @override
  Future<Map<String, Object?>> read(String url) async => result;
  @override
  void cancel() { stopped = true; }
}

void main() {
  test('API diagnostic closes request client after failure without retry', () async {
    final client = TrackedClient();
    final diagnostics = ConnectorDiagnostics(
      ai: FailingAi(), clientFactory: () => client,
    );
    await expectLater(diagnostics.testAi(
      const ConnectorConfig(apiKey: 'fixture', modelId: 'model'),
    ), throwsStateError);
    expect(client.closed, isTrue);
  });

  test('browser diagnostic is independent of AI and closes browser', () async {
    final page = Page({'text': 'public page'});
    final diagnostics = ConnectorDiagnostics(browserFactory: () => page);
    await diagnostics.testBrowser('https://example.com/');
    expect(page.stopped, isTrue);
  });

  test('browser diagnostic rejects empty extraction and invalid URL', () async {
    final page = Page({'text': ''});
    final diagnostics = ConnectorDiagnostics(browserFactory: () => page);
    await expectLater(diagnostics.testBrowser('https://example.com/'), throwsStateError);
    expect(page.stopped, isTrue);
    await expectLater(diagnostics.testBrowser('http://example.com/'), throwsA(anything));
  });
}
