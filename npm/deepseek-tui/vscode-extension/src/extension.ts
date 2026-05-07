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
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

interface AcpResponse {
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

  async start(): Promise<void> {
    const config = vscode.workspace.getConfiguration('deepseek');
    const binaryPath = config.get<string>('binaryPath', 'deepseek');

    this.process = spawn(binaryPath, ['serve', '--acp'], {
      stdio: ['pipe', 'pipe', 'pipe'],
      cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath,
    });

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
        const msg: AcpResponse = JSON.parse(line);
        const resolve = this.pending.get(msg.id);
        if (resolve) {
          this.pending.delete(msg.id);
          resolve(msg);
        }
      } catch {
        // Non-JSON line (status messages, etc.)
      }
    });

    this.output.appendLine('DeepSeek TUI ACP client started');
  }

  private async send(method: string, params?: Record<string, unknown>): Promise<AcpResponse> {
    if (!this.process?.stdin) {
      throw new Error('ACP client not started');
    }

    const id = this.nextId++;
    const request: AcpRequest = { id, method, params };

    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.process!.stdin!.write(JSON.stringify(request) + '\n');
    });
  }

  async newSession(): Promise<string> {
    const resp = await this.send('session/new');
    return (resp.result as { session_id: string }).session_id;
  }

  async sendPrompt(
    sessionId: string,
    prompt: string,
    context?: string,
  ): Promise<string> {
    const resp = await this.send('session/prompt', {
      session_id: sessionId,
      prompt,
      context,
    });
    return (resp.result as { response: string }).response;
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
  client = new AcpClient();
  await client.start();

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

      // Show result in a new untitled document
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
        { location: vscode.ProgressLocation.Notification, title: 'DeepSeek is analyzing...' },
        async () => {
          const response = await client!.sendPrompt(
            sessionId,
            `Fix any bugs or issues in this ${language} code. Return ONLY the fixed code, no explanation:\n\`\`\`${language}\n${selection}\n\`\`\``,
          );

          // Extract code block from response
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

  vscode.window.showInformationMessage('DeepSeek TUI extension activated');
}

export async function deactivate(): Promise<void> {
  await client?.stop();
  client = null;
}
