import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:wex/features/conversation/widgets/conversation_composer.dart';
import 'package:wex/features/conversation/widgets/conversation_tools.dart';

void main() {
  testWidgets('composer leaves draft acceptance and clearing to its owner', (tester) async {
    final draft = TextEditingController(text: 'unsent');
    addTearDown(draft.dispose);
    String? submitted;
    await tester.pumpWidget(MaterialApp(home: Scaffold(
      body: ConversationComposer(
        draft: draft, busy: false,
        onSend: () => submitted = draft.text, onStop: null,
      ),
    )));
    await tester.tap(find.byKey(const ValueKey('send-message')));
    expect(submitted, 'unsent');
    expect(draft.text, 'unsent');
    draft.clear();
    await tester.pump();
    expect(find.text('unsent'), findsNothing);
  });

  testWidgets('editing tool targets requests permission only after explicit toggle', (tester) async {
    final path = TextEditingController();
    final url = TextEditingController();
    addTearDown(path.dispose);
    addTearDown(url.dispose);
    final requests = <String>[];
    await tester.pumpWidget(MaterialApp(home: Scaffold(
      body: ListView(children: [ConversationTools(
        pathController: path, urlController: url,
        repository: 'owner/repo', branch: 'work/test', busy: false,
        canRead: false, canPropose: false, browserEnabled: false,
        onFile: (read, propose) => requests.add('file:$read:$propose'),
        onBrowser: (enabled) => requests.add('browser:$enabled'),
      )]),
    )));
    await tester.tap(find.text('本对话工具授权'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.widgetWithText(TextField, '仓库内已有文件路径，如 README.md'), 'file.txt',
    );
    await tester.enterText(
      find.widgetWithText(TextField, '指定公开 HTTPS 网页（不带查询参数）'),
      'https://example.com/',
    );
    expect(requests, isEmpty);
    await tester.ensureVisible(find.text('允许 AI 读取所选文件'));
    await tester.tap(find.text('允许 AI 读取所选文件'));
    expect(requests, ['file:true:false']);
    expect(path.text, 'file.txt');
    expect(url.text, 'https://example.com/');
  });
}
