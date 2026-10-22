import 'dart:convert';
import 'dart:typed_data';

/// Bounded JSON syntax plus duplicate-key/depth validation, shared by bridges.
Object? decodeStrictJson(Uint8List bytes) {
  if (bytes.isEmpty || bytes.length > 65536) {
    throw const FormatException('Invalid bridge JSON');
  }
  final text = utf8.decode(bytes, allowMalformed: false);
  _StrictKeys(text).check();
  return jsonDecode(text);
}

// Only a bounded duplicate-key/depth precheck. JSON syntax remains owned by
// dart:convert; decoded string tokens make escaped-equivalent keys identical.
final class _StrictKeys {
  _StrictKeys(this.text);
  final String text;
  int i = 0;
  String get peek => i < text.length ? text[i] : '';
  Never bad() => throw const FormatException('Invalid bridge JSON');

  void whitespace() {
    while (i < text.length && ' \t\n\r'.contains(text[i])) { i++; }
  }

  void expect(String c) {
    whitespace();
    if (peek != c) bad();
    i++;
  }

  String string() {
    whitespace();
    if (peek != '"') bad();
    final start = i++;
    while (i < text.length) {
      if (text[i] == '\\') { i += 2; continue; }
      if (text[i++] == '"') return jsonDecode(text.substring(start, i)) as String;
    }
    bad();
  }

  void value(int depth) {
    if (depth > 16) bad();
    whitespace();
    if (peek == '{') {
      i++;
      whitespace();
      final keys = <String>{};
      if (peek == '}') { i++; return; }
      while (true) {
        if (!keys.add(string())) bad();
        expect(':');
        value(depth + 1);
        whitespace();
        if (peek == '}') { i++; return; }
        expect(',');
      }
    } else if (peek == '[') {
      i++;
      whitespace();
      if (peek == ']') { i++; return; }
      while (true) {
        value(depth + 1);
        whitespace();
        if (peek == ']') { i++; return; }
        expect(',');
      }
    } else if (peek == '"') {
      string();
    } else {
      final start = i;
      while (i < text.length && !',]} \t\r\n'.contains(text[i])) { i++; }
      if (i == start) bad();
    }
  }

  void check() {
    value(0);
    whitespace();
    if (i != text.length) bad();
  }
}
