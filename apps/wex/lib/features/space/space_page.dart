import 'package:flutter/material.dart';

class SpacePage extends StatelessWidget {
  const SpacePage({super.key});

  @override
  Widget build(BuildContext context) {
    return const Center(
      child: Padding(
        padding: EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.workspaces_outline, size: 52),
            SizedBox(height: 16),
            Text('暂无空间', style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600)),
            SizedBox(height: 8),
            Text('空间与工作上下文将在后续接入。', textAlign: TextAlign.center),
          ],
        ),
      ),
    );
  }
}
