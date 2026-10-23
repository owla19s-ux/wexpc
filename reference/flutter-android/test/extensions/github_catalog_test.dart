import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:github/github.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';

void main() {
  test('SDK lists accessible repositories and branches without a preselected target', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final requests = <String>[];
    server.listen((request) async {
      requests.add('${request.method} ${request.uri.path}');
      request.response.headers.contentType = ContentType.json;
      if (request.uri.path == '/user/repos') {
        expect(request.uri.queryParameters['type'], 'all');
        request.response.write(jsonEncode([
          {'full_name': 'owner/repo', 'name': 'repo'},
          {'full_name': 'team/another', 'name': 'another'},
        ]));
      } else if (request.uri.path == '/repos/team/another/branches') {
        request.response.write(jsonEncode([
          {'name': 'main'}, {'name': 'feature/one'},
        ]));
      } else {
        request.response.statusCode = 404;
        request.response.write('{}');
      }
      await request.response.close();
    });
    final extension = AndroidGitHubExtension(createClient: (token) => GitHub(
      endpoint: 'http://127.0.0.1:${server.port}',
      auth: Authentication.withToken(token),
    ));
    expect(await extension.listRepositories(const ConnectorConfig(
      githubToken: 'fixture', repository: '', branch: '',
    )), ['owner/repo', 'team/another']);
    expect(await extension.listBranches(const ConnectorConfig(
      githubToken: 'fixture', repository: 'team/another', branch: '',
    )), ['main', 'feature/one']);
    expect(requests, ['GET /user/repos', 'GET /repos/team/another/branches']);
  });

  test('catalog output is bounded without removing manual target entry', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      request.response.headers.contentType = ContentType.json;
      request.response.write(jsonEncode(List.generate(110,
        (i) => {'full_name': 'owner/repo$i', 'name': 'repo$i'})));
      await request.response.close();
    });
    final extension = AndroidGitHubExtension(createClient: (token) => GitHub(
      endpoint: 'http://127.0.0.1:${server.port}',
      auth: Authentication.withToken(token),
    ));
    final result = await extension.listRepositories(const ConnectorConfig(
      githubToken: 'fixture', repository: '', branch: '',
    ));
    expect(result, hasLength(100));
    expect(result.last, 'owner/repo99');
  });

  test('disabled catalog access does not create an SDK client', () async {
    var created = false;
    final extension = AndroidGitHubExtension(createClient: (_) {
      created = true;
      throw StateError('must not create client');
    });
    await expectLater(extension.listRepositories(const ConnectorConfig(
      githubToken: 'fixture', githubAvailable: false,
    )), throwsStateError);
    expect(created, isFalse);
  });
}
