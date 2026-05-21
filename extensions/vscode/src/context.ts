export function getEditorContext(): { filePath: string; selection: string; language: string } | null {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return null;
  return {
    filePath: editor.document.uri.fsPath,
    selection: editor.document.getText(editor.selection) || '',
    language: editor.document.languageId,
  };
}

export function buildContextPrefix(ctx: ReturnType<typeof getEditorContext>): string {
  if (!ctx) return '';
  let prefix = `File: ${ctx.filePath}\n`;
  if (ctx.selection) prefix += `Selected code (${ctx.language}):\n\`\`\`${ctx.language}\n${ctx.selection}\n\`\`\`\n`;
  return prefix;
}

// @-mention autocomplete provider for open files
export function registerAtMentionProvider(context: vscode.ExtensionContext) {
  vscode.languages.registerCompletionItemProvider({ scheme: 'deepseek-composer' }, {
    provideCompletionItems() {
      return vscode.workspace.textDocuments.map(doc =>
        new vscode.CompletionItem('@' + vscode.workspace.asRelativePath(doc.uri), vscode.CompletionItemKind.File)
      );
    }
  }, '@');
}
