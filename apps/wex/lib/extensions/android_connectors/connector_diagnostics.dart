import 'package:dart_openai/dart_openai.dart';
import 'package:http/http.dart' as http;
import 'ai_extension.dart';
import 'browser_extension.dart';
import 'connector_config.dart';
import 'github_extension.dart';

/// Explicit user diagnostics; does not save config, history, or grant tools.
class ConnectorDiagnostics {
  ConnectorDiagnostics({
    AndroidAiExtension? ai,
    AndroidGitHubExtension? github,
    AndroidBrowserExtension Function()? browserFactory,
    http.Client Function()? clientFactory,
  }) : _ai = ai ?? AndroidAiExtension(),
       _github = github ?? const AndroidGitHubExtension(),
       _browserFactory = browserFactory ?? AndroidBrowserExtension.new,
       _clientFactory = clientFactory ?? http.Client.new;

  final AndroidAiExtension _ai;
  final AndroidGitHubExtension _github;
  final AndroidBrowserExtension Function() _browserFactory;
  final http.Client Function() _clientFactory;

  http.Client? _activeClient;
  AndroidBrowserExtension? _activeBrowser;
  bool _disposed = false;

  void dispose() {
    _disposed = true;
    _activeClient?.close();
    _activeBrowser?.cancel();
  }

  void _checkOpen() {
    if (_disposed) throw StateError('诊断已关闭');
  }

  Future<void> testAi(ConnectorConfig config) async {
    _checkOpen();
    if (!config.hasAi) throw StateError('AI 未启用或配置不完整');
    final client = _clientFactory();
    _activeClient = client;
    try {
      final reply = await _ai.complete(config, [
        OpenAIChatCompletionChoiceMessageModel(
          role: OpenAIChatMessageRole.user,
          content: [
            OpenAIChatCompletionChoiceMessageContentItemModel.text(
              'Connection test. Reply only OK.',
            ),
          ],
        ),
      ], const [], client);
      _checkOpen();
      if ((reply.toolCalls ?? []).isNotEmpty ||
          !(reply.content ?? []).any((part) => (part.text ?? '').trim().isNotEmpty)) {
        throw StateError('测试未获得文本回复');
      }
    } finally {
      client.close();
      if (identical(_activeClient, client)) _activeClient = null;
    }
  }

  Future<void> testGitHub(ConnectorConfig config) async {
    _checkOpen();
    await _github.checkConnection(config);
    _checkOpen();
  }

  Future<void> testBrowser(String url) async {
    _checkOpen();
    // Validate before starting platform work; no model/provider is involved.
    final target = BrowserTarget.parse(url).toString();
    final browser = _browserFactory();
    _activeBrowser = browser;
    try {
      final result = await browser.read(target);
      _checkOpen();
      if (result['text'] is! String || (result['text'] as String).trim().isEmpty) {
        throw StateError('网页未返回可读取文字');
      }
    } finally {
      browser.cancel();
      if (identical(_activeBrowser, browser)) _activeBrowser = null;
    }
  }
}
