# deepseek-tui

Install and run the `deepseek` and `deepseek-tui` binaries from GitHub release artifacts.

## Install

```bash
npm install -g deepseek-tui
# or
pnpm add -g deepseek-tui
```

For project-local usage:

```bash
npm install deepseek-tui
npx deepseek-tui --help
```

`postinstall` downloads platform binaries into `bin/downloads/` and exposes
`deepseek` and `deepseek-tui` commands.

## First run

```bash
deepseek login --api-key "YOUR_DEEPSEEK_API_KEY"
deepseek doctor
deepseek
```

The `deepseek` facade and `deepseek-tui` binary share `~/.deepseek/config.toml`
for DeepSeek auth and default model settings. Common TUI commands are available
directly through the facade, including `deepseek doctor`, `deepseek models`,
`deepseek sessions`, and `deepseek resume --last`.

The app talks to DeepSeek's documented OpenAI-compatible Chat Completions API.
Set `DEEPSEEK_BASE_URL` only if you need the China endpoint or DeepSeek beta
features such as strict tool mode, chat prefix completion, or FIM completion.

## Supported platforms

- Linux x64
- macOS x64 / arm64
- Windows x64

Other platform/architecture combinations are not supported and will fail during install.

## Configuration

- Default binary version comes from `deepseekBinaryVersion` in `package.json`.
- Set `DEEPSEEK_TUI_VERSION` or `DEEPSEEK_VERSION` to override the release version.
- Set `DEEPSEEK_TUI_GITHUB_REPO` or `DEEPSEEK_GITHUB_REPO` to override the source repo (defaults to `Hmbown/DeepSeek-TUI`).
- Set `DEEPSEEK_TUI_FORCE_DOWNLOAD=1` to force download even when the cached binary is already present.
- Set `DEEPSEEK_TUI_DISABLE_INSTALL=1` to skip install-time download.

## Release integrity

- `npm publish` runs a release-asset check to ensure all required binary assets
  exist for the target GitHub release before publishing.
- Install-time downloads are verified against the release checksum manifest before
  the wrapper marks them executable.
- Set `DEEPSEEK_TUI_RELEASE_BASE_URL` to point the installer at a local or
  staged release-asset directory for smoke tests.
