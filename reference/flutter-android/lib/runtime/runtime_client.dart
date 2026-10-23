import 'runtime_models.dart';
export 'runtime_models.dart';

abstract interface class RuntimeClient {
  Future<RuntimeReply> invoke(RuntimeRequest request);
  Future<void> close();
}
