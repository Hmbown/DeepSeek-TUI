#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
# shellcheck source=scripts/release/crates.sh
source "${script_dir}/crates.sh"

usage() {
  cat <<'EOF'
usage: scripts/release/check-published.sh [--allow-npm-binary-mismatch] [VERSION]

Verifies that a release version is visible on both npm and crates.io.
Defaults VERSION to the workspace version in Cargo.toml.

Use --allow-npm-binary-mismatch only for npm packaging-only releases where
the npm package intentionally points at an older GitHub binary release.
EOF
}

allow_npm_binary_mismatch=0
version=""

while (($# > 0)); do
  case "$1" in
    --allow-npm-binary-mismatch)
      allow_npm_binary_mismatch=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ -n "${version}" ]]; then
        usage >&2
        exit 2
      fi
      version="$1"
      ;;
  esac
  shift
done

cd "${repo_root}"

if [[ -z "${version}" ]]; then
  version="$(grep -E '^version = "' Cargo.toml | head -n1 | sed -E 's/^version = "([^"]+)".*/\1/')"
fi

if [[ -z "${version}" ]]; then
  echo "Could not determine release version." >&2
  exit 1
fi

fail=0

echo "Checking published release ${version}..."

if npm_version="$(npm view "deepseek-tui@${version}" version 2>/dev/null)"; then
  echo "npm deepseek-tui@${npm_version} is published."
else
  echo "npm deepseek-tui@${version} is not published." >&2
  fail=1
fi

if npm_binary_version="$(npm view "deepseek-tui@${version}" deepseekBinaryVersion 2>/dev/null)"; then
  if [[ "${npm_binary_version}" == "${version}" ]]; then
    echo "npm deepseekBinaryVersion=${npm_binary_version}."
  elif [[ "${allow_npm_binary_mismatch}" == "1" ]]; then
    echo "npm deepseekBinaryVersion=${npm_binary_version} (allowed packaging-only mismatch)."
  else
    echo "npm deepseekBinaryVersion=${npm_binary_version}, expected ${version}." >&2
    fail=1
  fi
elif [[ "${allow_npm_binary_mismatch}" == "1" ]]; then
  echo "npm deepseekBinaryVersion is absent (allowed packaging-only mismatch)."
else
  echo "npm deepseekBinaryVersion is absent for deepseek-tui@${version}." >&2
  fail=1
fi

for crate in "${release_crates[@]}"; do
  if curl -fsSL "https://crates.io/api/v1/crates/${crate}/${version}" >/dev/null 2>&1; then
    echo "crates.io ${crate}@${version} is published."
  else
    echo "crates.io ${crate}@${version} is not published." >&2
    fail=1
  fi
done

if [[ "${fail}" == "0" ]]; then
  echo "Published release OK: npm deepseek-tui@${version} and ${#release_crates[@]} crates are visible."
fi

exit "${fail}"
