export 'browser_target.dart';

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';
import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'browser_target.dart';

/// Anonymous GET, pinned to prevalidated public DNS. WebView receives offline
/// HTML with all page scripts/resources blocked; only our DOM observation runs.
class AndroidBrowserExtension {
  HttpClient? _http;
  HeadlessInAppWebView? _view;
  bool _cancelled = false;

  void cancel() {
    _cancelled = true;
    _http?.close(force: true);
  }

  Future<Map<String, Object?>> read(String selectedUrl) async {
    final uri = BrowserTarget.parse(selectedUrl);
    final addresses = await InternetAddress.lookup(
      uri.host,
      type: InternetAddressType.IPv4,
    ).timeout(const Duration(seconds: 10));
    if (_cancelled ||
        addresses.isEmpty ||
        addresses.any((a) => !BrowserTarget.isPublic(a))) {
      throw StateError('网页地址不可用或不属于公开网络');
    }
    final client = HttpClient();
    client.connectionTimeout = const Duration(seconds: 15);
    client.findProxy = (_) => 'DIRECT';
    client.connectionFactory = (url, proxy, port) =>
        Socket.startConnect(addresses.first, 443);
    _http = client;
    try {
      final request = await client
          .getUrl(uri)
          .timeout(const Duration(seconds: 15));
      request.followRedirects = false;
      request.headers.set(HttpHeaders.acceptHeader, 'text/html,text/plain');
      final response = await request.close().timeout(
        const Duration(seconds: 20),
      );
      if (response.statusCode != 200)
        throw StateError('网页未返回 200；重定向须由人重新选择地址');
      final type = response.headers.contentType?.mimeType;
      if (type != 'text/html' && type != 'text/plain')
        throw StateError('只支持公开 HTML 或文本网页');
      final bytes = BytesBuilder(copy: false);
      final watch = Stopwatch()..start();
      await for (final part in response.timeout(const Duration(seconds: 15))) {
        if (_cancelled ||
            watch.elapsed > const Duration(seconds: 30) ||
            bytes.length + part.length > 512 * 1024) {
          throw StateError('已停止或网页超过 512KiB');
        }
        bytes.add(part);
      }
      final html = utf8.decode(bytes.takeBytes());
      if (type == 'text/plain')
        return {
          'url': uri.toString(),
          'text': _clip(html),
          'untrusted_content': true,
        };
      final result = Completer<Map<String, Object?>>();
      final view = HeadlessInAppWebView(
        initialData: InAppWebViewInitialData(
          data:
              '<meta http-equiv="Content-Security-Policy" content="default-src &#39;none&#39;; script-src &#39;none&#39;; form-action &#39;none&#39;; frame-src &#39;none&#39;">$html',
        ),
        initialSettings: InAppWebViewSettings(
          incognito: true,
          cacheEnabled: false,
          domStorageEnabled: false,
          blockNetworkLoads: true,
          blockNetworkImage: true,
          allowFileAccess: false,
          allowContentAccess: false,
          allowFileAccessFromFileURLs: false,
          allowUniversalAccessFromFileURLs: false,
          javaScriptCanOpenWindowsAutomatically: false,
          supportMultipleWindows: false,
          useShouldOverrideUrlLoading: true,
          useShouldInterceptRequest: true,
        ),
        shouldOverrideUrlLoading: (_, __) async =>
            NavigationActionPolicy.CANCEL,
        shouldInterceptRequest: (_, __) async =>
            WebResourceResponse(data: Uint8List(0)),
        onPermissionRequest: (_, request) async => PermissionResponse(
          resources: request.resources,
          action: PermissionResponseAction.DENY,
        ),
        onLoadStop: (controller, _) async {
          if (result.isCompleted) return;
          try {
            final text = await controller.evaluateJavascript(
              source:
                  '(document.body ? document.body.innerText : "").slice(0,12000)',
            );
            if (_cancelled) throw StateError('已停止');
            result.complete({
              'url': uri.toString(),
              'title': await controller.getTitle(),
              'text': _clip(text is String ? text : ''),
              'untrusted_content': true,
            });
          } catch (_) {
            if (!result.isCompleted) result.completeError(StateError('网页观察失败'));
          }
        },
      );
      _view = view;
      await view.run();
      return await result.future.timeout(const Duration(seconds: 15));
    } finally {
      client.close(force: true);
      _http = null;
      final view = _view;
      _view = null;
      if (view != null) await view.dispose();
    }
  }

  String _clip(String text) =>
      text.length > 12000 ? text.substring(0, 12000) : text;
}
