# DeepSeek TUI Release Runbook

This runbook is the source of truth for shipping Rust crates, GitHub release assets,
and the `deepseek-tui` npm wrapper.

Current packaging note:
- `deepseek-tui` is the live runtime and TUI package shipped to users today.
- `deepseek-tui-core` is a supporting workspace crate for the extraction/parity effort, not a replacement for the shipping runtime.

## Canonical Publish Targets

- End-user crates:
  - `deepseek-tui`
  - `deepseek-tui-cli`
- Supporting crates published from this workspace:
  - `deepseek-config`
  - `deepseek-protocol`
  - `deepseek-state`
  - `deepseek-agent`
  - `deepseek-execpolicy`
  - `deepseek-hooks`
  - `deepseek-mcp`
  - `deepseek-tools`
  - `deepseek-core`
  - `deepseek-app-server`
  - `deepseek-tui-core`
- `deepseek-cli` on crates.io is an unrelated crate and is not part of this release flow.

## Version Coordination

- Rust crates inherit the shared workspace version from [Cargo.toml](../Cargo.toml).
- Internal path dependency versions should match the shared workspace version; stale older pins are release blockers once the workspace version moves.
- The npm wrapper version lives in [npm/deepseek-tui/package.json](../npm/deepseek-tui/package.json).
- `deepseekBinaryVersion` controls which GitHub release binaries the npm wrapper downloads.
- Packaging-only npm releases are allowed:
  - bump the npm package version
  - leave `deepseekBinaryVersion` pinned to the previously released Rust binaries
  - rerun `npm pack` smoke checks before `npm publish`

## Preflight

Run these from the repository root before cutting a tag:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo publish --dry-run --locked --allow-dirty -p deepseek-tui
./scripts/release/publish-crates.sh dry-run
```

`publish-crates.sh dry-run` performs a full `cargo publish --dry-run` for crates
without unpublished workspace dependencies and a packaging preflight for dependent
workspace crates. That avoids false negatives from crates.io not yet containing the
new workspace version while still validating package contents before publish.

For npm wrapper verification:

```bash
cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui
node scripts/release/prepare-local-release-assets.js
python3 -m http.server 8123 --directory target/npm-release-assets
cd npm/deepseek-tui
DEEPSEEK_TUI_FORCE_DOWNLOAD=1 DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npm pack
```

Then install the generated tarball in a clean temp directory and smoke the entrypoints:

```bash
tmpdir="$(mktemp -d)"
cd "${tmpdir}"
npm init -y
DEEPSEEK_TUI_FORCE_DOWNLOAD=1 DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npm install /path/to/deepseek-tui-*.tgz
DEEPSEEK_TUI_FORCE_DOWNLOAD=1 DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npx --no-install deepseek --help
DEEPSEEK_TUI_FORCE_DOWNLOAD=1 DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npx --no-install deepseek-tui --help
```

To exercise `npm run release:check` locally as well, regenerate the local asset
directory with a full asset matrix fixture before starting the server:

```bash
DEEPSEEK_TUI_PREPARE_ALL_ASSETS=1 node scripts/release/prepare-local-release-assets.js
cd npm/deepseek-tui
DEEPSEEK_TUI_VERSION=X.Y.Z DEEPSEEK_TUI_RELEASE_BASE_URL=http://127.0.0.1:8123/ npm run release:check
```

Set `DEEPSEEK_TUI_VERSION` to the npm package version you are verifying for that local run.

The CI workflow runs the same tarball install + smoke test on Linux and macOS.

## Rust Crates Release

1. Update the workspace version in [Cargo.toml](../Cargo.toml).
2. Tag the release as `vX.Y.Z`.
3. Let `.github/workflows/crates-publish.yml` verify the workspace version and dry-run each crate.
4. Publish crates in this order:
   - `deepseek-config`
   - `deepseek-protocol`
   - `deepseek-state`
   - `deepseek-agent`
   - `deepseek-execpolicy`
   - `deepseek-hooks`
   - `deepseek-mcp`
   - `deepseek-tools`
   - `deepseek-core`
   - `deepseek-app-server`
   - `deepseek-tui-core`
   - `deepseek-tui-cli`
   - `deepseek-tui`
5. Wait for each published crate version to appear on crates.io before publishing dependents.

The publish helper is idempotent for reruns: already-published crate versions are skipped.

## GitHub Release Assets

`.github/workflows/release.yml` builds these binaries:

- `deepseek-linux-x64`
- `deepseek-macos-x64`
- `deepseek-macos-arm64`
- `deepseek-windows-x64.exe`
- `deepseek-tui-linux-x64`
- `deepseek-tui-macos-x64`
- `deepseek-tui-macos-arm64`
- `deepseek-tui-windows-x64.exe`

The release job also uploads `deepseek-artifacts-sha256.txt`. The npm installer and
release verification script both depend on that checksum manifest.

## npm Wrapper Release

1. Set the npm package version in [npm/deepseek-tui/package.json](../npm/deepseek-tui/package.json).
2. Set `deepseekBinaryVersion` to the GitHub release tag that should supply binaries.
3. For GitHub Actions publishing, configure npm Trusted Publishing for:
   - Publisher: GitHub Actions
   - Repository: `Hmbown/DeepSeek-TUI`
   - Workflow filename: `release.yml`
4. If the GitHub release succeeded but npm publishing failed, rerun only npm
   publication from Actions → Release → Run workflow with the failed version.
   This keeps npm's single trusted publisher pointed at `release.yml`.
5. For local manual publication, run:

```bash
cd npm/deepseek-tui
npm pack
npm publish
```

`prepublishOnly` verifies that all expected release assets and the checksum manifest exist.
The tag release workflow publishes through npm Trusted Publishing, so it does
not use `NPM_TOKEN`. npm requires Node 22.14.0+ and npm 11.5.1+ for that OIDC
path; the workflow uses Node 24.

## Recovery and Rollback

- Crates publish partially:
  - rerun `./scripts/release/publish-crates.sh publish`
  - already-published crate versions will be skipped
- GitHub assets missing or checksum manifest incomplete:
  - fix `.github/workflows/release.yml`
  - retag or upload corrected assets before `npm publish`
- npm packaging-only problem:
  - bump only the npm package version
  - keep `deepseekBinaryVersion` on the last known-good Rust release
  - repack and republish the wrapper
- A bad npm publish cannot be overwritten:
  - publish a new npm version with corrected metadata or install logic
