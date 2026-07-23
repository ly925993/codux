import 'package:codux_flutter/i18n.dart';
import 'package:codux_flutter/theme/app_theme.dart';
import 'package:codux_flutter/widgets/components/project_files_panel.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  tearDown(() {
    CoduxTheme.brightness = Brightness.dark;
  });

  testWidgets('file preview keeps readable text in light app theme', (
    tester,
  ) async {
    // The surrounding app can be light while the source preview deliberately
    // retains its dark editor canvas.
    CoduxTheme.brightness = Brightness.light;
    final controller = CodeEditingController(text: '<h1>Codux</h1>');

    await tester.pumpWidget(
      MaterialApp(
        theme: buildAppTheme(brightness: Brightness.light),
        home: AppPreferences(
          accent: AccentChoices.cyan,
          locale: LocaleChoices.english,
          themeMode: ThemeMode.light,
          child: Scaffold(
            body: FileEditorView(
              path: '/repo/README.md',
              controller: controller,
              loading: false,
              saving: false,
              editing: false,
              editable: true,
              onClose: () {},
              onEdit: () {},
              onSave: () {},
            ),
          ),
        ),
      ),
    );

    final field = tester.widget<TextField>(find.byType(TextField));
    expect(field.style?.color, AppColors.terminalText);

    final span = controller.buildTextSpan(
      context: tester.element(find.byType(TextField)),
      style: field.style,
      withComposing: false,
    );
    expect(span.style?.color, AppColors.terminalText);
    expect(span.toPlainText(), '<h1>Codux</h1>');
  });
}
