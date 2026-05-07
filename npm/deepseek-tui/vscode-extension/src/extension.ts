/**
 * DeepSeek TUI — VS Code Extension
 *
 * Connects to the `deepseek serve --acp` process via the Agent Client
 * Protocol (ACP) over stdio. Provides inline AI assistance: explain code,
 * fix bugs, review changes, and open a chat panel.
 *
 * Architecture:
 *   VS Code → spawns `deepseek serve --acp` → reads/writes JSON-RPC over
 *   the child process's stdin/stdout. The ACP adapter handles session
 *   management, prompt routing, and context assembly.
 */

import * as vscode from 'vscode';
import { spawn, ChildProcess } from 'child_process';
import { ReadLine, createInterface } from 'readline';

// ── Types ────────────────────────────────────────────────────────────

interface AcpRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

interface AcpNotification {
  jsonrpc: '2.0';
  method: string;
  params?: Record<string, unknown>;
}

interface AcpResponse {
  jsonrpc: '2.0';
  id: number;
  result?: unknown;
  error?: { code: number; message: string };
}

// ── ACP Client ────────────────────────────────────────────────────────

class AcpClient {
  private process: ChildProcess | null = null;
  private reader: ReadLine | null = null;
  private nextId = 1;
  private pending = new Map<number, (resp: AcpResponse) => void>();
  private output = vscode.window.createOutputChannel('DeepSeek TUI');
  private notificationHandlers = new Map<
    string,
    Array<(params: Record<string, unknown>) => void>
  >();

  async start(): Promise<void> {
    const config = vscode.workspace.getConfiguration('deepseek');
    const binaryPath = config.get<string>('binaryPath', 'deepseek');

    try {
      this.process = spawn(binaryPath, ['serve', '--acp'], {
        stdio: ['pipe', 'pipe', 'pipe'],
        cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath,
      });
    } catch (err: unknown) {
      const message =
        err instanceof Error ? err.message : String(err);
      if (
        message.includes('ENOENT') ||
        message.includes('spawn') ||
        message.includes(binaryPath)
      ) {
        vscode.window.showErrorMessage(
          `DeepSeek TUI: Could not find the deepseek CLI binary at "${binaryPath}". ` +
            'Install it with `npm install -g deepseek-tui` or set the correct path ' +
            'in Settings → Extensions → DeepSeek TUI → Binary Path.',
        );
      } else {
        vscode.window.showErrorMessage(
          `DeepSeek TUI: Failed to start ACP server: ${message}`,
        );
      }
      return;
    }

    this.process.stderr?.on('data', (data: Buffer) => {
      this.output.append(data.toString());
    });

    this.process.on('exit', (code) => {
      this.output.appendLine(`DeepSeek ACP exited with code ${code}`);
      this.process = null;
    });

    this.reader = createInterface({ input: this.process.stdout! });
    this.reader.on('line', (line: string) => {
      try {
        const msg = JSON.parse(line);

        // JSON-RPC notification (no id field) — route to handlers
        if (msg.method && msg.id === undefined) {
          const handlers = this.notificationHandlers.get(msg.method);
          if (handlers) {
            for (const handler of handlers) {
              handler((msg.params ?? {}) as Record<string, unknown>);
            }
          }
          return;
        }

        // JSON-RPC response (has id field)
        if (msg.id !== undefined) {
          const resolve = this.pending.get(msg.id);
          if (resolve) {
            this.pending.delete(msg.id);
            resolve(msg as AcpResponse);
          }
        }
      } catch {
        // Non-JSON line (status messages, etc.)
      }
    });

    this.output.appendLine('DeepSeek TUI ACP client started');
  }

  /** Register a handler for a JSON-RPC notification method. */
  onNotification(
    method: string,
    handler: (params: Record<string, unknown>) => void,
  ): void {
    const handlers = this.notificationHandlers.get(method) ?? [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);
  }

  private async send(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<AcpResponse> {
    if (!this.process?.stdin) {
      throw new Error('ACP client not started');
    }

    const id = this.nextId++;
    const request: AcpRequest = { jsonrpc: '2.0', id, method, params };

    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.process!.stdin!.write(JSON.stringify(request) + '\n');
    });
  }

  async newSession(): Promise<string> {
    const resp = await this.send('session/new');
    return (resp.result as { sessionId: string }).sessionId;
  }

  /**
   * Send a prompt and collect the full response.
   *
   * The ACP protocol sends the LLM output via `session/update` notifications
   * and finishes with a JSON-RPC result containing `stopReason`. We listen
   * for notifications during the RPC call and accumulate the text.
   */
  async sendPrompt(
    sessionId: string,
    prompt: string,
    context?: string,
  ): Promise<string> {
    const chunks: string[] = [];

    const handler = (params: Record<string, unknown>) => {
      const update = params.update as Record<string, unknown> | undefined;
      const content = update?.content as Record<string, unknown> | undefined;
      if (content?.type === 'text' && typeof content.text === 'string') {
        chunks.push(content.text);
      }
    };

    this.onNotification('session/update', handler);

    try {
      await this.send('session/prompt', {
        sessionId,
        prompt,
        ...(context ? { context } : {}),
      });
    } finally {
      // Remove handler after this call completes
      const handlers = this.notificationHandlers.get('session/update');
      if (handlers) {
        const idx = handlers.indexOf(handler);
        if (idx !== -1) handlers.splice(idx, 1);
      }
    }

    return chunks.join('\n\n');
  }

  async stop(): Promise<void> {
    if (this.process) {
      this.process.stdin?.end();
      this.process.kill();
      this.process = null;
    }
  }
}

// ── Extension Activation ──────────────────────────────────────────────

let client: AcpClient | null = null;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel('DeepSeek TUI');
  output.appendLine('DeepSeek TUI extension activating');

  client = new AcpClient();
  await client.start();

  output.appendLine('DeepSeek TUI extension activated');

  // Command: Explain selected code
  context.subscriptions.push(
    vscode.commands.registerCommand('deepseek.explainCode', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;

      const selection = editor.document.getText(editor.selection);
      if (!selection) {
        vscode.window.showWarningMessage('Select code to explain first');
        return;
      }

      const language = editor.document.languageId;
      const sessionId = await client!.newSession();
      const response = await client!.sendPrompt(
        sessionId,
        `Explain this ${language} code:\n\`\`\`${language}\n${selection}\n\`\`\``,
      );

      const doc = await vscode.workspace.openTextDocument({
        content: `# DeepSeek Explanation\n\n${response}`,
        language: 'markdown',
      });
      await vscode.window.showTextDocument(doc, { preview: true });
    }),
  );

  // Command: Fix selected code
  context.subscriptions.push(
    vscode.commands.registerCommand('deepseek.fixCode', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;

      const selection = editor.document.getText(editor.selection);
      if (!selection) {
        vscode.window.showWarningMessage('Select code to fix first');
        return;
      }

      const language = editor.document.languageId;
      const sessionId = await client!.newSession();

      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: 'DeepSeek is analyzing...',
        },
        async () => {
          const response = await client!.sendPrompt(
            sessionId,
            `Fix any bugs or issues in this ${language} code. Return ONLY the fixed code, no explanation:\n\`\`\`${language}\n${selection}\n\`\`\``,
          );

          const codeMatch = response.match(/```[\w]*\n([\s\S]*?)```/);
          const fixedCode = codeMatch ? codeMatch[1] : response;

          await editor.edit((editBuilder) => {
            editBuilder.replace(editor.selection, fixedCode);
          });
        },
      );
    }),
  );

  // Command: Review code
  context.subscriptions.push(
    vscode.commands.registerCommand('deepseek.reviewCode', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;

      const selection = editor.selection.isEmpty
        ? editor.document.getText()
        : editor.document.getText(editor.selection);

      const language = editor.document.languageId;
      const sessionId = await client!.newSession();
      const response = await client!.sendPrompt(
        sessionId,
        `Review this ${language} code for bugs, security issues, and style problems:\n\`\`\`${language}\n${selection}\n\`\`\``,
      );

      const doc = await vscode.workspace.openTextDocument({
        content: `# DeepSeek Code Review\n\n${response}`,
        language: 'markdown',
      });
      await vscode.window.showTextDocument(doc, { preview: true });
    }),
  );

  // Command: Start chat session
  context.subscriptions.push(
    vscode.commands.registerCommand('deepseek.startSession', async () => {
      const sessionId = await client!.newSession();
      output.appendLine(`Chat session started: ${sessionId}`);
      vscode.window.showInformationMessage(
        `DeepSeek chat session started (${sessionId.slice(0, 21)}...)`,
      );
    }),
  );

  // Command: Send selection to chat
  context.subscriptions.push(
    vscode.commands.registerCommand('deepseek.sendSelection', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;

      const selection = editor.document.getText(editor.selection);
      if (!selection) {
        vscode.window.showWarningMessage('Select text to send first');
        return;
      }

      const sessionId = await client!.newSession();
      const response = await client!.sendPrompt(sessionId, selection);

      const doc = await vscode.workspace.openTextDocument({
        content: response,
        language: 'markdown',
      });
      await vscode.window.showTextDocument(doc, { preview: true });
    }),
  );
}

export async function deactivate(): Promise<void> {
  await client?.stop();
  client = null;
}
