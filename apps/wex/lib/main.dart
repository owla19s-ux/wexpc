import 'package:flutter/material.dart';
import 'dart:async';
import 'dart:ui';
import 'app/wex_app.dart';
import 'platform/log_service.dart';
export 'app/wex_app.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  FlutterError.onError = (details) {
    FlutterError.presentError(details);
    unawaited(LogService.exception('FLUTTER_ERROR', details.stack ?? StackTrace.current));
  };
  PlatformDispatcher.instance.onError = (error, stack) {
    unawaited(LogService.exception('DART_UNCAUGHT', stack));
    return false;
  };
  unawaited(LogService.event('FLUTTER_START'));
  runApp(const WexApp());
}

