import 'package:flutter/material.dart';
import '../../extensions/android/extension_bundle_panel.dart';
import '../../extensions/android_connectors/connector_settings_panel.dart';
import '../../runtime/runtime_client.dart';
import 'runtime_diagnostics.dart';
import 'log_directory_panel.dart';
import 'storage_access_panel.dart';
import 'local_read_inspection_panel.dart';
import 'grant_inventory_panel.dart';

class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key, required this.runtime});
  final Future<RuntimeClient> Function() runtime;

  @override
  Widget build(BuildContext context) {
    const categories = [
      ('连接与服务', Icons.link, 'AI API、GitHub、AI 浏览器分别配置'),
      ('权限', Icons.shield_outlined, '尚无操作授权'),
      ('验证', Icons.fact_check_outlined, '离线运行时检查；不代表工作成果已验证'),
      ('系统', Icons.info_outline, 'WEX 0.1.3 · Android 扩展集成测试版'),
    ];
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        for (final category in categories)
          Card(
            child: ExpansionTile(
              leading: Icon(category.$2),
              title: Text(category.$1),
              childrenPadding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
              children: [
                Align(
                  alignment: Alignment.centerLeft,
                  child: Text(category.$3),
                ),
                if (category.$1 == '连接与服务') const AndroidExtensionBundlePanel(),
                if (category.$1 == '连接与服务') const ConnectorSettingsPanel(),
                if (category.$1 == '验证') RuntimeDiagnostics(runtime: runtime),
                if (category.$1 == '权限') const StorageAccessPanel(),
                if (category.$1 == '权限') const LocalReadInspectionPanel(),
                if (category.$1 == '权限') const GrantInventoryPanel(),
                if (category.$1 == '系统') const LogDirectoryPanel(),
              ],
            ),
          ),
      ],
    );
  }
}
