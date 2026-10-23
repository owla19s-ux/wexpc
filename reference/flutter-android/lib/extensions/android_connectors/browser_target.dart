import 'dart:io';

/// Exact page selected by the human. No model-supplied query or navigation.
class BrowserTarget {
  static Uri parse(String value) {
    final uri = Uri.tryParse(value.trim());
    if (uri == null ||
        uri.scheme != 'https' ||
        uri.port != 443 ||
        uri.userInfo.isNotEmpty ||
        uri.hasQuery ||
        uri.hasFragment ||
        InternetAddress.tryParse(uri.host) != null ||
        !RegExp(r'^[a-zA-Z0-9-]+(\.[a-zA-Z0-9-]+)+$').hasMatch(uri.host) ||
        [
          'localhost',
          'local',
          'internal',
          'test',
          'invalid',
          'example',
        ].contains(uri.host.split('.').last)) {
      throw const FormatException('网页须为公开 HTTPS 域名，不能带账号、查询参数、片段或特殊端口');
    }
    return uri;
  }

  /// Conservative IPv4-only transport for the first anonymous reader.
  static bool isPublic(InternetAddress address) {
    if (address.type != InternetAddressType.IPv4) return false;
    final b = address.rawAddress;
    if (b[0] == 0 ||
        b[0] == 10 ||
        b[0] == 127 ||
        b[0] >= 224 ||
        (b[0] == 100 && b[1] >= 64 && b[1] <= 127) ||
        (b[0] == 169 && b[1] == 254) ||
        (b[0] == 172 && b[1] >= 16 && b[1] <= 31) ||
        (b[0] == 192 &&
            (b[1] == 0 || b[1] == 168 || b[1] == 88 || (b[1] == 2))) ||
        (b[0] == 198 && (b[1] == 18 || b[1] == 19 || b[1] == 51)) ||
        (b[0] == 203 && b[1] == 0))
      return false;
    return true;
  }
}
