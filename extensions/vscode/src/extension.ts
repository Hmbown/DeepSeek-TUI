import * as vscode from "vscode";
import { ChatPanel } from "./chatPanel";

export function activate(context: vscode.ExtensionContext): void {
  const chatPanel = new ChatPanel(context);

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(ChatPanel.viewType, chatPanel, {
      webviewOptions: { retainContextWhenHidden: true },
    })
  );
}

export function deactivate(): void {
  // Cleanup if needed.
}
