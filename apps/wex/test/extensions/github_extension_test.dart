import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:github/github.dart';
import 'package:wex/extensions/android_connectors/connector_config.dart';
import 'package:wex/extensions/android_connectors/github_extension.dart';

void main() {
  test('connection check only reads repository metadata', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final requests = <String>[];
    server.listen((request) async {
      requests.add('${request.method} ${request.uri.path}');
      await request.drain<void>();
      request.response.headers.contentType = ContentType.json;
      request.response.write(jsonEncode({'name': 'repo', 'default_branch': 'main'}));
      await request.response.close();
    });
    final extension = AndroidGitHubExtension(createClient: (token) => GitHub(
      endpoint: 'http://127.0.0.1:${server.port}',
      auth: Authentication.withToken(token),
    ));
    await extension.checkConnection(const ConnectorConfig(
      repository: 'owner/repo', branch: '', githubToken: 'fixture',
    ));
    expect(requests, ['GET /repos/owner/repo']);
  });

  test('disabled GitHub connection does not create a client', () async {
    var created = false;
    final extension = AndroidGitHubExtension(createClient: (_) {
      created = true;
      throw StateError('must not create client');
    });
    await expectLater(extension.checkConnection(const ConnectorConfig(
      repository: 'owner/repo', branch: 'work/test', githubToken: 'fixture',
      githubAvailable: false,
    )), throwsStateError);
    expect(created, isFalse);
  });

  for (final branch in ['work/test', 'main', 'master', 'develop']) {
  test('verifies the returned commit on $branch even when branch advances', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final requests = <String>[];
    const commitSha = '1111111111111111111111111111111111111111';
    const blobSha = '2222222222222222222222222222222222222222';
    const originalSha = '3333333333333333333333333333333333333333';
    const replacement = 'approved content\n';
    server.listen((request) async {
      requests.add('${request.method} ${request.uri}');
      request.response.headers.contentType = ContentType.json;
      if (request.method == 'GET' && request.uri.path == '/repos/owla19s-ux/WEX') {
        request.response.write(jsonEncode({'name': 'WEX', 'default_branch': 'main'}));
      } else if (request.method == 'PUT') {
        final body = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
        expect(body['sha'], originalSha);
        expect(body['branch'], branch);
        expect(utf8.decode(base64Decode(body['content'] as String)), replacement);
        request.response.statusCode = HttpStatus.ok;
        request.response.write(jsonEncode({
          'content': {'sha': blobSha, 'path': 'file.txt', 'type': 'file'},
          'commit': {'sha': commitSha},
        }));
      } else {
        final pinned = request.uri.queryParameters['ref'] == commitSha;
        request.response.write(jsonEncode({
          'type': 'file', 'name': 'file.txt', 'path': 'file.txt',
          'sha': pinned ? blobSha : '4444444444444444444444444444444444444444',
          'size': replacement.length, 'encoding': 'base64',
          'content': base64Encode(utf8.encode(pinned ? replacement : 'later edit')),
        }));
      }
      await request.response.close();
    });
    final extension = AndroidGitHubExtension(
      createClient: (token) => GitHub(
        endpoint: 'http://127.0.0.1:${server.port}',
        auth: Authentication.withToken(token),
      ),
    );
    final receipt = await extension.commitApprovedReplacement(
      ConnectorConfig(githubToken: 'test-token', branch: branch),
      RepoFileSnapshot(
        repository: 'owla19s-ux/WEX', branch: branch, path: 'file.txt',
        blobSha: originalSha, text: 'old content',
      ),
      replacement,
    );
    expect(receipt.commitSha, commitSha);
    expect(receipt.newBlobSha, blobSha);
    expect(receipt.verified, isTrue);
    expect(requests.length, 2);
    expect(requests.last, contains('ref=$commitSha'));
  });
  }
  test('rejects changed approval target before creating SDK client', () async {
    var clients = 0;
    final extension = AndroidGitHubExtension(createClient: (_) {
      clients++;
      throw StateError('must not create a client');
    });
    await expectLater(
      extension.commitApprovedReplacement(
        const ConnectorConfig(githubToken: 'test-token', branch: 'work/other'),
        const RepoFileSnapshot(repository: 'owla19s-ux/WEX', branch: 'work/test',
          path: 'file.txt', blobSha: 'original', text: 'old'),
        'new',
      ),
      throwsStateError,
    );
    expect(clients, 0);
  });

  for (final status in [HttpStatus.conflict, HttpStatus.forbidden]) {
  test('does not retry a refused write ($status)', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var puts = 0;
    var gets = 0;
    server.listen((request) async {
      await request.drain<void>();
      request.response.headers.contentType = ContentType.json;
      if (request.method == 'PUT') {
        puts++;
        request.response.statusCode = status;
        request.response.write(jsonEncode({'message': 'sha does not match'}));
      } else {
        gets++;
        request.response.write(jsonEncode({'name': 'WEX', 'default_branch': 'main'}));
      }
      await request.response.close();
    });
    final extension = AndroidGitHubExtension(createClient: (token) => GitHub(
      endpoint: 'http://127.0.0.1:${server.port}',
      auth: Authentication.withToken(token),
    ));
    await expectLater(
      extension.commitApprovedReplacement(
        const ConnectorConfig(githubToken: 'test-token', branch: 'main'),
        const RepoFileSnapshot(repository: 'owla19s-ux/WEX', branch: 'main',
          path: 'file.txt', blobSha: 'original', text: 'old'),
        'new',
      ),
      throwsA(anything),
    );
    expect(puts, 1);
    expect(gets, 0);
  });


  }
}
