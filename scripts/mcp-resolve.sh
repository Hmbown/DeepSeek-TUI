#!/usr/bin/env bash
# Resolves latest installed version of a claude-mem or context-mode plugin.
# Usage: mcp-resolve.sh <plugin-name> <relative-script-path>
#   plugin-name: "claude-mem" | "context-mode"
#   relative-script-path: path inside the version directory
#
# Output: absolute path to the script (trailing newline)
# Exit: 1 if plugin not found

set -euo pipefail

PLUGIN_CACHE="${HOME}/.claude/plugins/cache"
PLUGIN_NAME="$1"
SCRIPT_PATH="$2"

case "${PLUGIN_NAME}" in
  claude-mem)
    base="${PLUGIN_CACHE}/thedotmack/claude-mem"
    ;;
  context-mode)
    base="${PLUGIN_CACHE}/context-mode/context-mode"
    ;;
  *)
    echo "mcp-resolve.sh: unknown plugin '${PLUGIN_NAME}'" >&2
    exit 1
    ;;
esac

if [[ ! -d "${base}" ]]; then
  echo "mcp-resolve.sh: plugin directory not found: ${base}" >&2
  exit 1
fi

latest=$(ls -d "${base}"/*/ 2>/dev/null | sort -V | tail -1)
if [[ -z "${latest}" ]]; then
  echo "mcp-resolve.sh: no version directories found under ${base}" >&2
  exit 1
fi

echo "${latest}${SCRIPT_PATH}"
