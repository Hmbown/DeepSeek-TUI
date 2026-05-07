# DeepSeek TUI — VS Code Companion Extension

This extension provides a single command to launch [DeepSeek TUI](https://github.com/Hmbown/DeepSeek-TUI) in VS Code's integrated terminal.

**Note:** This is a companion extension for the standalone `deepseek` binary. It does **not** re-implement the TUI inside VS Code — it launches the native terminal application alongside your editor.

## Requirements

- [DeepSeek TUI](https://github.com/Hmbown/DeepSeek-TUI) installed (`npm i -g deepseek-tui` or `cargo install deepseek-tui-cli deepseek-tui`)
- VS Code 1.90+

## Commands

| Command | Description |
|---|---|
| `DeepSeek TUI: Launch in terminal` | Opens the integrated terminal and launches `deepseek` |

## Publishing

```bash
npm install
npm run publish
```

See [scripts/vsc-publish.sh](../../scripts/vsc-publish.sh) for the automated pipeline.
