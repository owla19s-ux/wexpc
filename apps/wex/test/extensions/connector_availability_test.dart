import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/conversation_controller.dart';

class DisabledStore extends ConnectorConfigStore {
  @override
  Future<ConnectorConfig> load() async => const ConnectorConfig(
    apiKey: 'fixture', modelId: 'fixture-model',
    githubToken: 'fixture', repository: 'owner/repo', branch: 'work/test',
    githubAvailable: false, browserAvailable: false,
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  test('legacy v1 keeps availability without granting tool consent', () {
    final legacy = ConnectorConfig.fromJson({'version': 1});
    expect(legacy.aiAvailable, isTrue);
    expect(legacy.githubAvailable, isTrue);
    expect(legacy.browserAvailable, isTrue);
    expect(legacy.hasAi, isFalse);
    expect(legacy.hasGitHub, isFalse);
  });

  test('availability flags survive secure save and reload', () async {
    final store = ConnectorConfigStore();
    await store.save(const ConnectorConfig(
      apiKey: 'fixture', modelId: 'fixture-model', githubToken: 'fixture',
      repository: 'owner/repo', branch: 'work/test',
      aiAvailable: false, githubAvailable: false, browserAvailable: false,
    ));
    final c = await store.load();
    expect(c.hasAi, isFalse);
    expect(c.hasGitHub, isFalse);
    expect(c.browserAvailable, isFalse);
    expect(c.withHistoryScope('scope').browserAvailable, isFalse);
    expect(c.withHistoryScope('scope').aiAvailable, isFalse);
    expect(c.withHistoryScope('scope').githubAvailable, isFalse);
  });

  test('disabling browser preserves confidential history scope', () async {
    final store = ConnectorConfigStore();
    await store.save(const ConnectorConfig(apiKey: 'fixture', modelId: 'model'));
    final before = await store.load();
    await store.save(const ConnectorConfig(
      apiKey: 'fixture', modelId: 'model', browserAvailable: false,
    ));
    expect((await store.load()).historyScope, before.historyScope);
  });

  test('loaded disabled modules refuse new grants while AI remains usable', () async {
    final host = AndroidConversationController(store: DisabledStore());
    addTearDown(host.dispose);
    await host.load();
    expect(host.config!.hasAi, isTrue);
    expect(() => host.selectBrowser('https://example.com/', true), throwsStateError);
    expect(() => host.selectFile('README.md', read: true, propose: true), throwsStateError);
    expect(host.browserEnabled, isFalse);
    expect(host.canRead, isFalse);
    expect(host.canPropose, isFalse);
    // Revocation remains available with invalid/empty targets.
    host.selectBrowser('', false);
    host.selectFile('', read: false, propose: false);
  });
}
