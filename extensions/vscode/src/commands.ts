import * as vscode from 'vscode';

export function registerCommands(context: vscode.ExtensionContext) {
  context.subscriptions.push(
    vscode.commands.registerCommand('deepseek.openChat', () => {
      vscode.commands.executeCommand('deepseek.chatView.focus');
    }),
    vscode.commands.registerCommand('deepseek.askAboutSelection', () => {
      const editor = vscode.window.activeTextEditor;
      const selection = editor?.document.getText(editor.selection);
      vscode.commands.executeCommand('deepseek.chatView.focus');
      if (selection) vscode.commands.executeCommand('deepseek.sendMessage', `Explain this code:\n\`\`\`\n${selection}\n\`\`\``);
    }),
    vscode.commands.registerCommand('deepseek.applyToSelection', () => {
      vscode.commands.executeCommand('deepseek.chatView.focus');
    }),
    vscode.commands.registerCommand('deepseek.runTest', () => {
      vscode.commands.executeCommand('deepseek.sendMessage', 'Run the tests for the current file and report results.');
    }),
  );
}
