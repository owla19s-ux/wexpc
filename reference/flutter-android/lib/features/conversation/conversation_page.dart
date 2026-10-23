import 'package:flutter/material.dart';
import 'dart:io';
import '../../extensions/android_connectors/conversation_panel.dart';

class ConversationPage extends StatelessWidget {
  const ConversationPage({super.key, required this.draft});

  final TextEditingController draft;

  @override
  Widget build(BuildContext context) {
    if (Platform.isAndroid) return AndroidConversationPanel(draft: draft);
    return Column(
      children: [
        const Expanded(
          child: Center(
            child: SingleChildScrollView(
              padding: EdgeInsets.all(24),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(Icons.chat_bubble_outline, size: 40),
                  SizedBox(height: 12),
                  Text(
                    '尚未连接 AI',
                    style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600),
                  ),
                  SizedBox(height: 8),
                  Text('连接配置将在后续开放。草稿不会发送。', textAlign: TextAlign.center),
                ],
              ),
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 12, 16),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('message-draft'),
                  controller: draft,
                  minLines: 1,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    hintText: '写下消息（尚未发送）',
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              const IconButton(
                key: ValueKey('send-message'),
                tooltip: '连接后才能发送',
                onPressed: null,
                icon: Icon(Icons.arrow_upward),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
