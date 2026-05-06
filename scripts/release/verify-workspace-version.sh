#!/usr/bin/env bash
set -euo pipefail

expected_version="${1:-}"
if [[ -z "${expected_version}" && "${GITHUB_REF:-}" == refs/tags/v* ]]; then
  expected_version="${GITHUB_REF#refs/tags/v}"
fi

if [[ -z "${expected_version}" ]]; then
  echo "usage: $0 <version>" >&2
  exit 1
fi

python3 - "${expected_version}" <<'PY'
import json
import subprocess
import sys

expected = sys.argv[1]
metadata = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"])
)
workspace_members = set(metadata["workspace_members"])
packages = [pkg for pkg in metadata["packages"] if pkg["id"] in workspace_members]
mismatches = [
    f"{pkg['name']}={pkg['version']}" for pkg in packages if pkg["version"] != expected
]

if mismatches:
    print(f"Tag version {expected} does not match all workspace crates:", file=sys.stderr)
    for item in mismatches:
        print(f"  - {item}", file=sys.stderr)
    sys.exit(1)

print(f"Verified {len(packages)} workspace packages at version {expected}")
PY
