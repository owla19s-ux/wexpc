import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/connector_settings_panel.dart';

void main() {
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  testWidgets('saving API does not submit an invalid GitHub draft', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: Scaffold(
      body: SingleChildScrollView(child: ConnectorSettingsPanel()),
    )));
    await tester.pumpAndSettle();
    await tester.tap(find.text('GitHub'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('connector-field-4')), 'invalid draft');
    await tester.tap(find.text('GitHub'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('AI API'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('connector-field-1')), 'fixture-key');
    await tester.enterText(find.byKey(const ValueKey('connector-field-2')), 'fixture-model');
    await tester.ensureVisible(find.text('保存 AI API'));
    await tester.tap(find.text('保存 AI API'));
    await tester.pumpAndSettle();
    final saved = await ConnectorConfigStore().load();
    expect(saved.apiKey, 'fixture-key');
    expect(saved.modelId, 'fixture-model');
    expect(saved.repository, 'owla19s-ux/WEX');
    await tester.ensureVisible(find.text('AI API'));
    await tester.tap(find.text('AI API'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('GitHub'));
    await tester.pumpAndSettle();
    final field = tester.widget<TextField>(find.byKey(const ValueKey('connector-field-4')));
    expect(field.controller!.text, 'invalid draft');
  });

  testWidgets('saving GitHub preserves unsaved API draft and saved API identity', (tester) async {
    final store = ConnectorConfigStore();
    await store.save(const ConnectorConfig(apiKey: 'saved-key', modelId: 'saved-model'));
    await tester.pumpWidget(const MaterialApp(home: Scaffold(
      body: SingleChildScrollView(child: ConnectorSettingsPanel()),
    )));
    await tester.pumpAndSettle();
    await tester.tap(find.text('AI API'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('connector-field-0')), 'invalid endpoint');
    await tester.tap(find.text('AI API'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('GitHub'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('connector-field-3')), 'repo-fixture');
    await tester.enterText(find.byKey(const ValueKey('connector-field-4')), 'owner/another');
    await tester.enterText(find.byKey(const ValueKey('connector-field-5')), 'work/test');
    await tester.ensureVisible(find.text('保存 GitHub'));
    await tester.tap(find.text('保存 GitHub'));
    await tester.pumpAndSettle();
    final saved = await store.load();
    expect(saved.apiKey, 'saved-key');
    expect(saved.modelId, 'saved-model');
    expect(saved.apiBaseUrl, 'https://api.openai.com/v1');
    expect(saved.repository, 'owner/another');
  });
}
