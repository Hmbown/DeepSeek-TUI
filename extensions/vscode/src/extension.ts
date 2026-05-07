import * as vscode from 'vscode';
import { DeepSeekServer } from './server';

let server: DeepSeekServer | undefined;

/**
 * Activate the DeepSeek VS Code extension.
 *
 * On activation:
 * 1. Instantiates `DeepSeekServer` and calls `start()`.
 * 2. Stores the resolved port in `context.workspaceState` for downstream
 *    features (status bar, webview, inline edits) to discover the server URL.
 * 3. Registers a disposer that stops the server on deactivation.
 */
export async function activate(context: vscode.ExtensionContext): Promise<void> {
  server = new DeepSeekServer();

  try {
    const port = await server.start();
    await context.workspaceState.update('deepseekPort', port);

    console.log(`DeepSeek server running at ${server.baseUrl}`);

    // Register a one-shot deactivation listener.
    context.subscriptions.push({
      dispose: () => {
        void server?.stop();
      },
    });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.error('Failed to start DeepSeek server:', msg);
    // The binary-not-found case already shows an error dialog via
    // DeepSeekServer#start().  Don't double-notify.
  }
}

/**
 * Deactivate the DeepSeek VS Code extension.
 *
 * Kills the spawned deepseek process if it is still running.
 */
export async function deactivate(): Promise<void> {
  if (server) {
    await server.stop();
    server = undefined;
  }
}
