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
