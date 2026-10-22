import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/extensions/android_connectors/browser_extension.dart';

void main() {
  test('only explicitly chosen public HTTPS page URLs are accepted', () {
    expect(
      BrowserTarget.parse('https://docs.flutter.dev/').host,
      'docs.flutter.dev',
    );
    for (final url in [
      'file:///a',
      'http://example.com',
      'https://localhost/',
      'https://127.0.0.1/',
      'https://example.com/?secret=value',
      'https://user:pass@example.com/',
      'https://example.com:8443/',
    ]) {
      expect(
        () => BrowserTarget.parse(url),
        throwsFormatException,
        reason: url,
      );
    }
  });
  test('DNS private, metadata, shared and reserved ranges fail closed', () {
    for (final ip in [
      '127.0.0.1',
      '10.0.0.1',
      '172.16.0.1',
      '192.168.1.1',
      '169.254.169.254',
      '100.64.0.1',
      '0.0.0.0',
      '224.0.0.1',
      '198.18.0.1',
      '::1',
    ]) {
      expect(BrowserTarget.isPublic(InternetAddress(ip)), isFalse, reason: ip);
    }
    expect(BrowserTarget.isPublic(InternetAddress('8.8.8.8')), isTrue);
  });
}
