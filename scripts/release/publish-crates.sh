#!/usr/bin/env bash
set -euo pipefail

mode="${1:-dry-run}"
case "${mode}" in
  dry-run|publish) ;;
  *)
    echo "usage: $0 [dry-run|publish]" >&2
    exit 1
    ;;
esac

packages=(
  deepseek-config
  deepseek-protocol
  deepseek-state
  deepseek-agent
  deepseek-execpolicy
  deepseek-hooks
  deepseek-mcp
  deepseek-tools
  deepseek-core
  deepseek-app-server
  deepseek-tui-core
  deepseek-tui-cli
  deepseek-tui
)

workspace_version="$(
  python3 - <<'PY'
import json
import subprocess

metadata = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"])
)
workspace_members = set(metadata["workspace_members"])
for pkg in metadata["packages"]:
    if pkg["id"] in workspace_members:
        print(pkg["version"])
        break
PY
)"

package_has_workspace_deps() {
  local package_name="$1"
  python3 - "${package_name}" <<'PY'
import json
import subprocess
import sys

package_name = sys.argv[1]
metadata = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"])
)
workspace_ids = set(metadata["workspace_members"])
workspace_packages = {
    pkg["name"]: pkg for pkg in metadata["packages"] if pkg["id"] in workspace_ids
}
package = workspace_packages[package_name]
has_workspace_dep = any(
    dep.get("path") and dep["name"] in workspace_packages
    for dep in package["dependencies"]
)
print("1" if has_workspace_dep else "0")
PY
}

crate_version_exists() {
  local crate_name="$1"
  local crate_version="$2"
  curl -fsSL "https://crates.io/api/v1/crates/${crate_name}/${crate_version}" >/dev/null 2>&1
}

wait_for_crate_version() {
  local crate_name="$1"
  local crate_version="$2"
  local attempts=30

  for ((attempt = 1; attempt <= attempts; attempt += 1)); do
    if crate_version_exists "${crate_name}" "${crate_version}"; then
      return 0
    fi
    echo "Waiting for ${crate_name} ${crate_version} to appear on crates.io (${attempt}/${attempts})..."
    sleep 10
  done

  echo "Timed out waiting for ${crate_name} ${crate_version} to appear on crates.io" >&2
  return 1
}

for package in "${packages[@]}"; do
  echo "::group::${mode} ${package}"
  if [[ "${mode}" == "dry-run" ]]; then
    if [[ "$(package_has_workspace_deps "${package}")" == "1" ]]; then
      cargo package --allow-dirty --locked --list -p "${package}" >/dev/null
      echo "Verified package contents for ${package}; full crates.io dry-run requires workspace dependencies at ${workspace_version} to be published first."
    else
      cargo publish --dry-run --locked --allow-dirty -p "${package}"
    fi
  else
    if crate_version_exists "${package}" "${workspace_version}"; then
      echo "Skipping ${package} ${workspace_version}; already published."
    else
      cargo publish --locked -p "${package}"
      wait_for_crate_version "${package}" "${workspace_version}"
    fi
  fi
  echo "::endgroup::"
done
