import 'package:dart_openai/dart_openai.dart';
import 'package:http/http.dart' as http;
import 'connector_config.dart';

class AndroidAiExtension {
  Future<OpenAIChatCompletionChoiceMessageModel> complete(
    ConnectorConfig config,
    List<OpenAIChatCompletionChoiceMessageModel> messages,
    List<OpenAIToolModel> tools,
    http.Client client,
  ) async {
    config.validate();
    if (!config.hasAi) throw StateError('AI 配置不完整');
    final uri = Uri.parse(config.apiBaseUrl.trim());
    final parts = uri.pathSegments.where((p) => p.isNotEmpty).toList();
    final version = parts.isEmpty ? 'v1' : parts.removeLast();
    final base = uri
        .replace(pathSegments: parts)
        .toString()
        .replaceFirst(RegExp(r'/$'), '');
    final sdk = OpenAIClient(
      apiKey: config.apiKey.trim(),
      baseUrl: base,
      version: version,
      retryPolicy: OpenAIRetryPolicy.none,
      requestsTimeOut: const Duration(seconds: 60),
    );
    final response = await sdk.chat.create(
      model: config.modelId.trim(),
      messages: messages,
      tools: tools.isEmpty ? null : tools,
      client: _NoRedirectClient(client),
    );
    if (response.choices.isEmpty) throw StateError('AI 响应为空');
    return response.choices.first.message;
  }
}

/// A configured endpoint cannot redirect a credential-bearing request elsewhere.
class _NoRedirectClient extends http.BaseClient {
  _NoRedirectClient(this.inner);
  final http.Client inner;
  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) {
    request.followRedirects = false;
    return inner.send(request);
  }
}
