import 'package:flutter/material.dart';
import 'package:dart_openai/dart_openai.dart' show OpenAIClient;
import 'package:github/github.dart' show GitHub;
import 'package:flutter_inappwebview/flutter_inappwebview.dart'
    show HeadlessInAppWebView;
import 'package:flutter_secure_storage/flutter_secure_storage.dart'
    show FlutterSecureStorage;

/// Confirms package linkage only. A linked SDK is NOT a configured, authorized,
/// executed or Android-device-verified connection/tool.
class AndroidExtensionBundlePanel extends StatelessWidget {
  const AndroidExtensionBundlePanel({super.key});

  @override
  Widget build(BuildContext context) {
    final sdkTypes = <Type>[
      OpenAIClient,
      GitHub,
      HeadlessInAppWebView,
      FlutterSecureStorage,
    ];

    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Android 扩展依赖（${sdkTypes.length} 项已引入）',
            style: Theme.of(context).textTheme.titleSmall,
          ),
          const SizedBox(height: 8),
          const Text('AI API · dart_openai 8.0.0 — 对话接线已实现，实际联通待验证'),
          const Text('GitHub · github 9.26.0 — 所选文件读取/提议/审批接线，实际仓库待验证'),
          const Text('AI 浏览器 · flutter_inappwebview 6.1.5 — 指定公开网页静态只读，设备待验证'),
          const Text('凭证支持 · flutter_secure_storage 10.3.4 — 配置保存已接入，设备待验证'),
          const SizedBox(height: 6),
          const Text(
            '本批为 Android 接线首片，不能将编译/测试等同真实服务或真机联通。'
            '工具权限须在对话内单独开启；关闭浏览器不影响 GitHub。',
            style: TextStyle(fontSize: 12),
          ),
        ],
      ),
    );
  }
}
