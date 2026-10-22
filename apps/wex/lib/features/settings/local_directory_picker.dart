import 'package:flutter/material.dart';
import '../../platform/local_directory_browser.dart';

/// Android all-files direct-path browser. This is NOT a SAF document picker.
class LocalDirectoryPicker extends StatefulWidget {
  const LocalDirectoryPicker({super.key});
  @override
  State<LocalDirectoryPicker> createState() => _LocalDirectoryPickerState();
}

class _LocalDirectoryPickerState extends State<LocalDirectoryPicker> {
  LocalDirectoryListing? _listing;
  String? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _load(null);
  }

  Future<void> _load(String? path) async {
    if (_busy) return;
    setState(() { _busy = true; _error = null; });
    try {
      final result = await LocalDirectoryBrowser.browse(path);
      if (mounted) setState(() => _listing = result);
    } on LocalDirectoryBrowserException catch (error) {
      if (mounted) setState(() => _error = error.code);
    } catch (_) {
      if (mounted) setState(() => _error = 'BROWSER_UNAVAILABLE');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final listing = _listing;
    return AlertDialog(
      title: const Text('选择本地目录'),
      content: SizedBox(
        width: double.maxFinite,
        height: 390,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('仅浏览已有系统权限允许的目录；不是 SAF 授权。'),
            if (listing != null)
              SelectableText(listing.current,
                  key: const ValueKey('browser-current')),
            if (_error != null)
              Text('无法列出目录：$_error',
                  key: const ValueKey('browser-error')),
            if (_busy) const LinearProgressIndicator(),
            if (listing != null)
              TextButton.icon(
                onPressed: _busy || listing.parent == null
                    ? null : () => _load(listing.parent),
                icon: const Icon(Icons.arrow_upward),
                label: const Text('上一级'),
              ),
            Expanded(
              child: listing == null
                  ? const Center(child: Text('正在读取可访问目录…'))
                  : ListView.builder(
                      itemCount: listing.directories.length,
                      itemBuilder: (context, index) {
                        final folder = listing.directories[index];
                        return ListTile(
                          dense: true,
                          leading: const Icon(Icons.folder_outlined),
                          title: Text(folder.name),
                          onTap: _busy ? null : () => _load(folder.path),
                        );
                      },
                    ),
            ),
            if (listing?.truncated == true)
              const Text('目录较多，仅显示前 128 项。'),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context),
            child: const Text('取消')),
        FilledButton(
          onPressed: !_busy && _error == null && listing != null
              ? () => Navigator.pop(context, listing.current) : null,
          child: const Text('选择此目录'),
        ),
      ],
    );
  }
}
