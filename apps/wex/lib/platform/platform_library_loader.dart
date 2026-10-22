import 'dart:io';
import '../runtime/runtime_models.dart';

// The override is for trusted host/test composition, never user/UI input.
abstract final class PlatformLibraryLoader {
  static String resolve({String? overridePath}) {
    if (overridePath != null && overridePath.isNotEmpty) return overridePath;
    if (Platform.isAndroid) return 'libwex_runtime.so';
    if (Platform.isLinux) return 'libwex_runtime.so';
    if (Platform.isWindows) return 'wex_runtime.dll';
    if (Platform.isMacOS) return 'libwex_runtime.dylib';
    throw const RuntimeTransportException('LIBRARY_LOAD_FAILED');
  }
}
