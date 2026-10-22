import 'dart:async';
import 'dart:isolate';
import 'dart:typed_data';
import '../platform/platform_library_loader.dart';
import 'native_call.dart';
import 'runtime_client.dart';

abstract final class PlatformRuntimeClient {
  static Future<RuntimeClient> open({String? libraryPath}) async {
    final path = PlatformLibraryLoader.resolve(overridePath:libraryPath);
    final client = _IsolateClient();
    try {
      await Isolate.spawn(_worker, [client._events.sendPort, path],
          onError:client._events.sendPort, onExit:client._events.sendPort,
          errorsAreFatal:true, debugName:'wex-offline-runtime');
      await client._ready.future;
      return client;
    } on RuntimeTransportException {
      await client._closed.future;
      rethrow;
    } catch (_) {
      client._fail('NATIVE_INTERNAL');
      client._exit();
      throw const RuntimeTransportException('NATIVE_INTERNAL');
    }
  }
}

enum _State { opening, open, closing, closed, failed }

final class _IsolateClient implements RuntimeClient {
  _IsolateClient() {
    _ready.future.ignore();
    _events.listen(_event);
  }
  final _events = ReceivePort();
  final _ready = Completer<void>();
  final _closed = Completer<void>();
  _State _state = _State.opening;
  SendPort? _commands;
  Completer<RuntimeReply>? _pending;
  RuntimeRequest? _request;
  int _token = 0;

  void _fail(String code) {
    _state = _State.failed;
    if (!_ready.isCompleted) _ready.completeError(RuntimeTransportException(code));
    final pending = _pending;
    _pending = null;
    _request = null;
    if (pending != null && !pending.isCompleted) {
      pending.completeError(RuntimeTransportException(code));
    }
    _commands?.send({'kind':'close'});
  }

  void _exit() {
    if (_closed.isCompleted) return;
    if (_state != _State.closing || _pending != null) {
      _fail('NATIVE_INTERNAL');
    } else {
      _state = _State.closed;
    }
    _events.close();
    _closed.complete();
  }

  void _event(dynamic message) {
    if (message == null) { _exit(); return; }
    if (message is! Map) { _fail('NATIVE_INTERNAL'); return; }
    if (message['kind'] == 'startupError') {
      _fail(message['code'] as String);
      return;
    }
    if (message['kind'] == 'ready') {
      if (_state != _State.opening) { _fail('NATIVE_INTERNAL'); return; }
      _commands = message['port'] as SendPort;
      _state = _State.open;
      _ready.complete();
      return;
    }
    if (message['kind'] == 'closed') return;
    if (_state == _State.failed || _state == _State.closed) return;
    if (message['kind'] != 'result' || _pending == null || message['token'] != _token) {
      _fail('NATIVE_INTERNAL');
      return;
    }
    final pending = _pending!;
    final request = _request!;
    _pending = null;
    _request = null;
    try {
      if (message['code'] != null) throw RuntimeTransportException(message['code'] as String);
      pending.complete(RuntimeCodec.decode(message['bytes'] as Uint8List, request));
    } catch (error) {
      pending.completeError(error is RuntimeTransportException
          ? error : const RuntimeTransportException('NATIVE_INTERNAL'));
    }
    if (_state == _State.closing) _commands!.send({'kind':'close'});
  }

  @override
  Future<RuntimeReply> invoke(RuntimeRequest request) {
    if (_state == _State.closed || _state == _State.closing) {
      return Future.error(const RuntimeTransportException('RUNTIME_CLOSED'));
    }
    if (_state != _State.open) {
      return Future.error(const RuntimeTransportException('NATIVE_INTERNAL'));
    }
    if (_pending != null) return Future.error(const RuntimeTransportException('RUNTIME_BUSY'));
    final Uint8List bytes;
    try { bytes = RuntimeCodec.encode(request); }
    catch (error) { return Future.error(error); }
    final pending = Completer<RuntimeReply>();
    _pending = pending;
    _request = request;
    _token++;
    _commands!.send({'kind':'call','token':_token,'bytes':bytes});
    return pending.future;
  }

  @override
  Future<void> close() {
    if (_state == _State.open) {
      _state = _State.closing;
      if (_pending == null) _commands!.send({'kind':'close'});
    }
    return _closed.future;
  }
}

void _worker(List<dynamic> args) {
  final reply = args[0] as SendPort;
  final commands = ReceivePort();
  late NativeCall native;
  try { native = NativeCall.open(args[1] as String); }
  on RuntimeTransportException catch (e) {
    reply.send({'kind':'startupError','code':e.code});
    commands.close();
    return;
  } catch (_) {
    reply.send({'kind':'startupError','code':'NATIVE_INTERNAL'});
    commands.close();
    return;
  }
  commands.listen((dynamic message) {
    if (message is! Map) throw StateError('Invalid internal runtime message');
    if (message['kind'] == 'close') {
      commands.close();
      reply.send({'kind':'closed'});
      return;
    }
    try {
      final bytes = native.run(message['bytes'] as Uint8List);
      // All native buffers have already been freed by NativeCall.run.
      reply.send({'kind':'result','token':message['token'],'bytes':bytes});
    } on RuntimeTransportException catch (e) {
      reply.send({'kind':'result','token':message['token'],'code':e.code});
    } catch (_) {
      reply.send({'kind':'result','token':message['token'],'code':'NATIVE_INTERNAL'});
    }
  });
  reply.send({'kind':'ready','port':commands.sendPort});
}
