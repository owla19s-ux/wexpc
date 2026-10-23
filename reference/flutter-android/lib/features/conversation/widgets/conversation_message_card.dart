import 'package:flutter/material.dart';

/// Presentation only: the host supplies the existing message role and text.
class ConversationMessageCard extends StatelessWidget {
  const ConversationMessageCard({super.key, required this.role, required this.text});
  final String role;
  final String text;

  @override
  Widget build(BuildContext context) => Card(
    child: Padding(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            role == 'user' ? '你' : 'AI / 执行回执',
            style: Theme.of(context).textTheme.labelMedium,
          ),
          const SizedBox(height: 6),
          SelectableText(text),
        ],
      ),
    ),
  );
}
