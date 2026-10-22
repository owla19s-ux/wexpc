final class LocalReadException implements Exception {
  const LocalReadException(this.code, {this.outcomeUnknown = false});
  final String code;
  final bool outcomeUnknown;
  @override
  String toString() => 'LocalReadException($code)';
}
