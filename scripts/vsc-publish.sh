#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# scripts/vsc-publish.sh — VS Code extension marketplace publishing pipeline
#
# Packages the companion extension under crates/vsc/ and publishes it to the
# VS Code Marketplace via @vscode/vsce.
#
# Usage:
#   ./scripts/vsc-publish.sh          # dry-run (packages .vsix locally)
#   ./scripts/vsc-publish.sh publish  # publish to marketplace
#
# Dependencies:
#   - npm / Node.js >= 18
#   - @vscode/vsce (installed via npm ci in crates/vsc/)
#   - VS Code Marketplace publisher token (VSCE_PAT env var, or ~/.vsce)
#
# Environment variables:
#   VSCE_PAT      — Personal Access Token for the VS Code Marketplace
#   VSCE_BASE     — Base URL for vsce (optional; default: marketplace)
# ---------------------------------------------------------------------------
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "${script_dir}/.." && pwd)"
vsc_dir="${project_root}/crates/vsc"

mode="${1:-dry-run}"
case "${mode}" in
  dry-run|publish) ;;
  *)
    echo "usage: $0 [dry-run|publish]" >&2
    exit 1
    ;;
esac

# ----- Preflight checks ---------------------------------------------------

if [[ ! -d "${vsc_dir}" ]]; then
  echo "Error: VS Code extension directory not found at ${vsc_dir}" >&2
  echo "Run from the project root or ensure crates/vsc/ exists." >&2
  exit 1
fi

if ! command -v npm &>/dev/null; then
  echo "Error: npm is required but not found on PATH." >&2
  exit 1
fi

# ----- Build the extension -------------------------------------------------

echo "::group::Install dependencies"
cd "${vsc_dir}"
npm ci --omit=dev --ignore-scripts 2>&1
npm install --no-save typescript @types/vscode @vscode/vsce 2>&1
echo "::endgroup::"

echo "::group::Compile TypeScript"
npx tsc -p ./tsconfig.json --noEmit false 2>&1
echo "::endgroup::"

# ----- Package / publish ---------------------------------------------------

case "${mode}" in
  dry-run)
    echo "::group::Package .vsix (dry-run)"
    npx vsce package --no-dependencies --out "${project_root}/deepseek-tui-vsc.vsix" 2>&1
    echo "Packaged: ${project_root}/deepseek-tui-vsc.vsix"
    echo "::endgroup::"
    ;;

  publish)
    if [[ -z "${VSCE_PAT:-}" && ! -f "${HOME}/.vsce/vsce-pat" ]]; then
      echo "Error: VSCE_PAT is not set and no ~/.vsce/vsce-pat found." >&2
      echo "Set VSCE_PAT or run 'vsce login hmbown' first." >&2
      exit 1
    fi

    echo "::group::Publish to VS Code Marketplace"
    npx vsce publish --no-dependencies 2>&1
    echo "Published deepseek-tui-vsc to the VS Code Marketplace."
    echo "::endgroup::"
    ;;
esac
