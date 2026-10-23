import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';
import 'package:wex/extensions/android_connectors/github_target_picker.dart';

class Catalog extends AndroidGitHubExtension {
  Catalog({this.failFirst = false});
  final bool failFirst;
  @override
  Future<List<String>> listRepositories(ConnectorConfig config) async =>
      ['owner/first', 'team/second'];
  @override
  Future<List<String>> listBranches(ConnectorConfig config) async {
    if (failFirst && config.repository == 'owner/first') {
      throw StateError('fixture detail must not appear');
    }
    expect(config.branch, isEmpty);
    return ['main', 'feature/one'];
  }
}

class DelayedCatalog extends Catalog {
  final branches = Completer<List<String>>();
  @override
  Future<List<String>> listBranches(ConnectorConfig config) => branches.future;
}

void main() {
  for (final failFirst in [false, true]) {
    testWidgets('human selection returns exact draft target; failedFirst=$failFirst',
      (tester) async {
      GitHubTarget? selected;
      final revision = ConnectorConfigStore.revisions.value;
      await tester.pumpWidget(MaterialApp(home: Builder(builder: (context) =>
        Scaffold(body: TextButton(
          onPressed: () async {
            selected = await showDialog<GitHubTarget>(
              context: context,
              builder: (_) => GitHubTargetPicker(
                config: const ConnectorConfig(githubToken: 'fixture'),
                extension: Catalog(failFirst: failFirst),
              ),
            );
          },
          child: const Text('open'),
        )),
      )));
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();
      if (failFirst) {
        await tester.tap(find.text('owner/first'));
        await tester.pumpAndSettle();
        expect(find.textContaining('fixture detail'), findsNothing);
        expect(find.text('main'), findsNothing);
        await tester.tap(find.text('返回仓库列表'));
        await tester.pumpAndSettle();
      }
      await tester.tap(find.text('team/second'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('main'));
      await tester.pumpAndSettle();
      expect(selected!.repository, 'team/second');
      expect(selected!.branch, 'main');
      expect(ConnectorConfigStore.revisions.value, revision);
    });
  }

  testWidgets('a stale repository tap cannot become a branch selection', (tester) async {
    final catalog = DelayedCatalog();
    GitHubTarget? selected;
    await tester.pumpWidget(MaterialApp(home: Builder(builder: (context) =>
      Scaffold(body: TextButton(
        onPressed: () async {
          selected = await showDialog<GitHubTarget>(
            context: context,
            builder: (_) => GitHubTargetPicker(
              config: const ConnectorConfig(githubToken: 'fixture'),
              extension: catalog,
            ),
          );
        },
        child: const Text('open'),
      )),
    )));
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    final first = tester.widget<ListTile>(find.widgetWithText(ListTile, 'owner/first'));
    final stale = tester.widget<ListTile>(find.widgetWithText(ListTile, 'team/second'));
    first.onTap!();
    stale.onTap!(); // Same rendered frame, before the loading UI has rebuilt.
    await tester.pump();
    expect(selected, isNull);
    expect(find.byType(GitHubTargetPicker), findsOneWidget);
    catalog.branches.complete(['main']);
    await tester.pumpAndSettle();
    final branch = tester.widget<ListTile>(find.widgetWithText(ListTile, 'main'));
    branch.onTap!();
    branch.onTap!(); // A second stale branch tap must not pop the parent route.
    await tester.pumpAndSettle();
    expect(find.text('open'), findsOneWidget);
    expect(selected!.repository, 'owner/first');
    expect(selected!.branch, 'main');
  });
}
