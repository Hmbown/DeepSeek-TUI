#!/usr/bin/env bash
# Fails CI if version state is inconsistent across the workspace, npm
# wrapper, and Cargo.lock. Run on every push/PR so silent drift can't ship.
#
# Checks performed:
#   1. No `crates/*/Cargo.toml` carries a literal `version = "x.y.z"`; every
#      crate must inherit `version.workspace = true`.
#   2. `npm/deepseek-tui/package.json` `version` matches the workspace
#      `version` in the root `Cargo.toml`.
#   3. Internal `deepseek-*` path dependency pins match the workspace version.
#   4. The TUI crate's packaged changelog copy matches root `CHANGELOG.md`.
#   5. `SECURITY.md` keeps the dedicated security contact.
#   6. `Cargo.lock` is in sync with the manifests (`cargo metadata --locked`
#      fails if not).
set -euo pipefail

cd "$(dirname "$0")/../.."

fail=0

# 1) Literal versions in crate manifests.
literals="$(grep -nE '^version = "' crates/*/Cargo.toml || true)"
if [[ -n "${literals}" ]]; then
  echo "::error::Crate manifests must use 'version.workspace = true', not literal versions:" >&2
  echo "${literals}" >&2
  fail=1
fi

# 2) Workspace ↔ npm package.json.
workspace_version="$(grep -E '^version = "' Cargo.toml | head -n1 | sed -E 's/^version = "([^"]+)".*/\1/')"
npm_version="$(node -p "require('./npm/deepseek-tui/package.json').version")"
if [[ "${workspace_version}" != "${npm_version}" ]]; then
  echo "::error::npm/deepseek-tui/package.json version (${npm_version}) does not match workspace Cargo.toml (${workspace_version})." >&2
  fail=1
fi

# 3) Internal path dependency pins.
internal_dep_drift="$(
  grep -nE 'deepseek-[a-z-]+[[:space:]]*=[[:space:]]*\{[^}]*version[[:space:]]*=[[:space:]]*"' crates/*/Cargo.toml \
    | grep -v "version[[:space:]]*=[[:space:]]*\"${workspace_version}\"" || true
)"
if [[ -n "${internal_dep_drift}" ]]; then
  echo "::error::Internal deepseek-* path dependency versions must match workspace version ${workspace_version}:" >&2
  echo "${internal_dep_drift}" >&2
  fail=1
fi

# 4) Packaged TUI changelog copy.
if ! cmp -s CHANGELOG.md crates/tui/CHANGELOG.md; then
  echo "::error::crates/tui/CHANGELOG.md must match root CHANGELOG.md for crates.io packaging." >&2
  echo "Run: cp CHANGELOG.md crates/tui/CHANGELOG.md" >&2
  fail=1
fi

# 5) Security contact guard.
security_email="security@deepseek-tui.com"
if ! grep -qF "${security_email}" SECURITY.md; then
  echo "::error::SECURITY.md must list ${security_email} as the security contact." >&2
  fail=1
fi
if grep -qF "hmbown.dev@gmail.com" SECURITY.md; then
  echo "::error::SECURITY.md must not use the personal fallback email; use ${security_email}." >&2
  fail=1
fi

# 6) Cargo.lock in sync.
if ! cargo metadata --locked --format-version 1 --no-deps >/dev/null 2>&1; then
  echo "::error::Cargo.lock is out of sync with the manifests. Run 'cargo update -p deepseek-tui' or 'cargo build' and commit the result." >&2
  fail=1
fi

if [[ "${fail}" -eq 0 ]]; then
  echo "Version state OK: workspace=${workspace_version}, npm=${npm_version}, lockfile in sync."
fi

exit "${fail}"
