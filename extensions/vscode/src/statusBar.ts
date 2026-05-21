import * as vscode from 'vscode';

export class DeepSeekStatusBar {
  private item: vscode.StatusBarItem;
  private mode: 'Plan' | 'Agent' | 'YOLO' = 'Agent';
  private model = 'deepseek-v4-flash';

  constructor(context: vscode.ExtensionContext) {
    this.item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    this.item.command = 'deepseek.pickModeAndModel';
    context.subscriptions.push(this.item);
    this.update();
    this.item.show();

    context.subscriptions.push(
      vscode.commands.registerCommand('deepseek.pickModeAndModel', async () => {
        const picked = await vscode.window.showQuickPick(
          ['Plan', 'Agent', 'YOLO'].flatMap(m =>
            ['deepseek-v4-flash', 'deepseek-v4-pro'].map(mdl => ({ label: `${m} · ${mdl}`, mode: m as any, model: mdl }))
          ),
          { placeHolder: 'Select mode and model' }
        );
        if (picked) { this.mode = picked.mode; this.model = picked.model; this.update(); }
      })
    );
  }

  private update() {
    this.item.text = `$(robot) ${this.mode} · ${this.model}`;
    this.item.tooltip = 'Click to switch DeepSeek mode and model';
  }
}
