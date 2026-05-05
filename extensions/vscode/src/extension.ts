import * as vscode from 'vscode';

let outputChannel: vscode.OutputChannel | undefined;
let statusBarItem: vscode.StatusBarItem | undefined;

export function activate(context: vscode.ExtensionContext): void {
	// Output channel for diagnostics and messages
	outputChannel = vscode.window.createOutputChannel('DeepSeek TUI');
	outputChannel.appendLine('DeepSeek TUI extension activated.');
	context.subscriptions.push(outputChannel);

	// Status bar item
	statusBarItem = vscode.window.createStatusBarItem(
		vscode.StatusBarAlignment.Right,
		100,
	);
	statusBarItem.text = '$(comment-discussion) DeepSeek';
	statusBarItem.tooltip = 'DeepSeek TUI — Click to open chat';
	statusBarItem.command = 'deepseek.openChat';
	statusBarItem.show();
	context.subscriptions.push(statusBarItem);

	// Register commands
	context.subscriptions.push(
		vscode.commands.registerCommand('deepseek.openChat', () => {
			outputChannel?.appendLine('[deepseek.openChat] — not yet implemented');
			vscode.window.showInformationMessage('DeepSeek: Open Chat (placeholder)');
		}),
	);

	context.subscriptions.push(
		vscode.commands.registerCommand('deepseek.askAboutSelection', () => {
			const editor = vscode.window.activeTextEditor;
			if (!editor || editor.selection.isEmpty) {
				vscode.window.showWarningMessage('DeepSeek: No selection.');
				return;
			}
			const text = editor.document.getText(editor.selection);
			outputChannel?.appendLine(`[deepseek.askAboutSelection] selection: ${text.slice(0, 200)}`);
			vscode.window.showInformationMessage('DeepSeek: Ask About Selection (placeholder)');
		}),
	);

	context.subscriptions.push(
		vscode.commands.registerCommand('deepseek.applyToSelection', () => {
			const editor = vscode.window.activeTextEditor;
			if (!editor || editor.selection.isEmpty) {
				vscode.window.showWarningMessage('DeepSeek: No selection.');
				return;
			}
			outputChannel?.appendLine('[deepseek.applyToSelection] — not yet implemented');
			vscode.window.showInformationMessage('DeepSeek: Apply to Selection (placeholder)');
		}),
	);

	context.subscriptions.push(
		vscode.commands.registerCommand('deepseek.runTest', () => {
			outputChannel?.appendLine('[deepseek.runTest] — not yet implemented');
			vscode.window.showInformationMessage('DeepSeek: Run Test (placeholder)');
		}),
	);
}

export function deactivate(): void {
	outputChannel?.appendLine('DeepSeek TUI extension deactivated.');
	outputChannel?.dispose();
	statusBarItem?.dispose();
}
