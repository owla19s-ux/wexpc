import 'package:flutter/material.dart';

/// The owner retains the draft and decides when a submission clears it.
class ConversationComposer extends StatelessWidget {
  const ConversationComposer({
    super.key,
    required this.draft,
    required this.busy,
    required this.onSend,
    required this.onStop,
  });

  final TextEditingController draft;
  final bool busy;
  final VoidCallback? onSend;
  final VoidCallback? onStop;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.all(12),
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
              hintText: '输入消息',
              border: OutlineInputBorder(),
            ),
          ),
        ),
        if (busy)
          IconButton(
            tooltip: '停止并撤销工具授权',
            onPressed: onStop,
            icon: const Icon(Icons.stop),
          )
        else
          IconButton(
            key: const ValueKey('send-message'),
            tooltip: '发送',
            onPressed: onSend,
            icon: const Icon(Icons.arrow_upward),
          ),
      ],
    ),
  );
}
