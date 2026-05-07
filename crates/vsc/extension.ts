import * as vscode from "vscode";

/**
 * Activate the DeepSeek TUI VS Code companion extension.
 *
 * Registers a command that launches the `deepseek` binary in the
 * integrated terminal.  This companion extension provides VS Code
 * integration points (commands, keybindings, tasks) on top of the
 * standalone deepseek-tui binary — it is **not** a full re-implementation
 * of the TUI inside VS Code.
 */
export function activate(context: vscode.ExtensionContext) {
  const disposable = vscode.commands.registerCommand("deepseek-tui.launch", () => {
    const terminal = vscode.window.createTerminal("DeepSeek TUI");
    terminal.show();
    terminal.sendText("deepseek");
  });

  context.subscriptions.push(disposable);
}

export function deactivate() {
  // Cleanup if needed in future versions.
}
