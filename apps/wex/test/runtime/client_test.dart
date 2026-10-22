import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/runtime/isolate_runtime_client.dart';
import 'package:wex/runtime/runtime_client.dart';

Matcher transport(String code) => isA<RuntimeTransportException>().having((e)=>e.code,'code',code);
void main() {
  final path = Platform.environment['WEX_TEST_LIBRARY']!;
  test('real Rust library returns correlated Unicode and three statuses', () async {
    final client = await PlatformRuntimeClient.open(libraryPath:path);
    addTearDown(client.close);
    expect((await client.invoke(RuntimeRequest.ping(requestId:'p1'))).value, 'pong');
    final echo = await client.invoke(RuntimeRequest.echo(requestId:'e1', text:'你好🌙\u0000'));
    expect(echo.value, '你好🌙\u0000');
    expect((await client.invoke(RuntimeRequest.demoFail(requestId:'f1'))).status, RuntimeStatus.error);
    expect((await client.invoke(RuntimeRequest.demoDeny(requestId:'d1'))).status, RuntimeStatus.denied);
  });
  test('busy and close preserve the in-flight result and reject later calls', () async {
    final client = await PlatformRuntimeClient.open(libraryPath:path);
    final pending = client.invoke(RuntimeRequest.ping(requestId:'p1'));
    final busy = client.invoke(RuntimeRequest.ping(requestId:'p2'));
    final closing = client.close();
    await expectLater(busy, throwsA(transport('RUNTIME_BUSY')));
    expect(identical(closing,client.close()), true);
    await expectLater(client.invoke(RuntimeRequest.ping(requestId:'p3')), throwsA(transport('RUNTIME_CLOSED')));
    expect((await pending).value,'pong');
    await closing;
    await client.close();
  });
  test('repeated calls and missing library failure complete without hangs', () async {
    await expectLater(PlatformRuntimeClient.open(libraryPath:'/missing/wex_library.so'), throwsA(transport('LIBRARY_LOAD_FAILED')));
    for (var n=0;n<3;n++) {
      final client = await PlatformRuntimeClient.open(libraryPath:path);
      for(var i=0;i<20;i++) {
        expect((await client.invoke(RuntimeRequest.ping(requestId:'p$i'))).value,'pong');
      }
      await client.close();
    }
  });

  test('startup ABI failures and failed calls still close and complete once', () async {
    final root = Platform.environment['WEX_TEST_FIXTURES']!;
    for (final mode in [1,2]) {
      await expectLater(PlatformRuntimeClient.open(libraryPath:'$root/fixture_$mode.so'),
          throwsA(transport('ABI_MISMATCH')));
    }
    for (final entry in {3:'MALFORMED_REPLY',4:'MALFORMED_REPLY',12:'NATIVE_PANIC'}.entries) {
      final client = await PlatformRuntimeClient.open(libraryPath:'$root/fixture_${entry.key}.so');
      try {
        await expectLater(client.invoke(RuntimeRequest.ping(requestId:'error1')),
            throwsA(transport(entry.value)));
        final pending = client.invoke(RuntimeRequest.ping(requestId:'error2'));
        final closing = client.close();
        await expectLater(pending,throwsA(transport(entry.value)));
        await closing;
        await expectLater(client.invoke(RuntimeRequest.ping(requestId:'closed')),
            throwsA(transport('RUNTIME_CLOSED')));
      } finally { await client.close(); }
    }
  }, timeout: const Timeout(Duration(seconds:20)));
}
