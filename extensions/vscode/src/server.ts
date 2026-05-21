import * as cp from 'child_process';
import * as vscode from 'vscode';

/**
 * Manages a local deepseek HTTP server process for the VS Code extension.
 *
 * 1. Finds the `deepseek` binary on PATH (via `which` / `where`).
 * 2. Spawns `deepseek serve --http --port 0` so the OS assigns a free port.
 * 3. Reads the actual port from the process stdout line:
 *    `Runtime API listening on http://127.0.0.1:<port>`
 * 4. Provides the resolved `baseUrl` for downstream API calls.
 */
export class DeepSeekServer {
  private process?: cp.ChildProcess;
  private _port?: number;

  /** Resolve the `deepseek` binary path from PATH. */
  private async findBinary(): Promise<string> {
    const isWin = process.platform === 'win32';
    const cmd = isWin ? 'where' : 'which';
    return new Promise<string>((resolve, reject) => {
      cp.exec(`${cmd} deepseek`, { timeout: 5_000 }, (err, stdout) => {
        if (err) {
          reject(new Error('deepseek binary not found on PATH'));
          return;
        }
        const path = stdout.trim().split('\n')[0];
        if (!path) {
          reject(new Error('deepseek binary not found on PATH'));
          return;
        }
        resolve(path);
      });
    });
  }

  /** Show an actionable error message when deepseek is not installed. */
  private showInstallPrompt(): void {
    const isWin = process.platform === 'win32';
    const lines: string[] = [
      'DeepSeek binary not found on PATH.',
      '',
      'Install it with one of:',
    ];
    if (isWin) {
      lines.push('  winget install deepseek');
    } else {
      lines.push('  brew install deepseek-tui/tap/deepseek');
    }
    lines.push('  cargo install deepseek-tui');
    if (!isWin) {
      lines.push('  npm install -g @deepseek/tui');
    }
    void vscode.window.showErrorMessage(lines.join('\n'), 'Install Guide').then((sel) => {
      if (sel === 'Install Guide') {
        void vscode.env.openExternal(
          vscode.Uri.parse('https://github.com/Hmbown/DeepSeek-TUI?tab=readme-ov-file#install'),
        );
      }
    });
  }

  /**
   * Start the `deepseek serve --http` process.
   * Resolves with the port number once the server reports it on stdout.
   */
  async start(): Promise<number> {
    if (this.process) {
      return this._port!;
    }

    let binaryPath: string;
    try {
      binaryPath = await this.findBinary();
    } catch {
      this.showInstallPrompt();
      throw new Error('deepseek binary not found on PATH');
    }

    return new Promise<number>((resolve, reject) => {
      const proc = cp.spawn(binaryPath, ['serve', '--http', '--port', '0'], {
        stdio: ['ignore', 'pipe', 'pipe'],
        // Inherit the parent environment so PATH, HOME, etc. are available.
        env: { ...process.env },
      });

      this.process = proc;

      let started = false;
      const timeout = setTimeout(() => {
        if (!started) {
          this.kill();
          reject(new Error('deepseek server failed to start within 15 s'));
        }
      }, 15_000);

      const onData = (chunk: string) => {
        if (started) return;
        // Look for the startup line:  Runtime API listening on http://<host>:<port>
        const match = chunk.match(/listening on http:\/\/[^:]+:(\d+)/);
        if (match) {
          started = true;
          clearTimeout(timeout);
          this._port = parseInt(match[1], 10);
          resolve(this._port);
        }
      };

      proc.stdout?.on('data', (data: Buffer) => onData(data.toString('utf8')));
      proc.stderr?.on('data', (data: Buffer) => onData(data.toString('utf8')));

      proc.on('error', (err) => {
        clearTimeout(timeout);
        if (!started) {
          reject(err);
        }
      });

      proc.on('exit', (code, signal) => {
        clearTimeout(timeout);
        if (!started) {
          reject(
            new Error(
              `deepseek server exited unexpectedly (code=${code}, signal=${signal})`,
            ),
          );
        }
      });
    });
  }

  /** Gracefully stop the server process. */
  async stop(): Promise<void> {
    this.kill();
  }

  /** The resolved base URL of the running server. */
  get baseUrl(): string {
    if (!this._port) {
      throw new Error('DeepSeekServer not started – call start() first');
    }
    return `http://localhost:${this._port}`;
  }

  /** The assigned port, or undefined if not yet started. */
  get port(): number | undefined {
    return this._port;
  }

  private kill(): void {
    if (this.process) {
      try {
        this.process.kill('SIGTERM');
      } catch {
        // Process may already be dead
      }
      this.process = undefined;
      this._port = undefined;
    }
  }
}
