# deepseek-tui

This package installs the `deepseek` and `deepseek-tui` binaries from the
`DeepSeek-TUI` GitHub release artifacts and exposes them as Node-compatible
console entry points.

## Install

```bash
npm install deepseek-tui
# or
pnpm add deepseek-tui
```

This runs `postinstall`, downloads the platform-specific binaries for version
`0.3.28`, and makes `deepseek` and `deepseek-tui` available on your PATH.

## Supported platforms

- Linux x64
- macOS x64 / arm64
- Windows x64

## Notes

- Binaries come directly from release assets in
  `https://github.com/Hmbown/DeepSeek-TUI/releases`.
- Set `DEEPSEEK_VERSION` to install a different release version (defaults to package version).
- Set `DEEPSEEK_GITHUB_REPO` to override the repo source (defaults to `Hmbown/DeepSeek-TUI`).
- Set `DEEPSEEK_TUI_FORCE_DOWNLOAD=1` to force download even when the cached binary is already present.
- Set `DEEPSEEK_TUI_DISABLE_INSTALL=1` to skip install-time download.
