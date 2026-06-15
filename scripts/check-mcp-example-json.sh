#!/usr/bin/env bash
# check-mcp-example-json.sh — Validate JSON blocks in MCP example READMEs.
#
# Extracts every fenced ```json``` block from the openclaw and pi example
# READMEs and validates each one with python3's json module. Fails if any
# block is malformed.
#
# Usage:
#   bash scripts/check-mcp-example-json.sh        # from repo root
#   CHECK_REPO_ROOT=/path/to/repo bash scripts/check-mcp-example-json.sh

set -euo pipefail

cd "${CHECK_REPO_ROOT:-$(dirname "$0")/..}"

READMES=(
  "examples/openclaw/README.md"
  "examples/pi/README.md"
)

FAIL=0

for readme in "${READMES[@]}"; do
  if [ ! -f "$readme" ]; then
    echo "FAIL: file not found: $readme" >&2
    FAIL=1
    continue
  fi

  echo "==> $readme"

  # Extract fenced ```json blocks using awk, print each to a temp file, then
  # validate with python3 -m json.tool (POSIX-safe, no heredoc inside YAML).
  block_index=0
  in_block=0
  current_block=""

  while IFS= read -r line; do
    if [ "$in_block" -eq 0 ] && printf '%s' "$line" | grep -qE '^```json[[:space:]]*$'; then
      in_block=1
      current_block=""
      continue
    fi
    if [ "$in_block" -eq 1 ] && printf '%s' "$line" | grep -qE '^```[[:space:]]*$'; then
      in_block=0
      block_index=$((block_index + 1))
      # Validate the extracted block
      if printf '%s' "$current_block" | python3 -m json.tool > /dev/null 2>&1; then
        echo "  ok: block ${block_index}"
      else
        echo "  FAIL: block ${block_index} is not valid JSON" >&2
        printf '%s' "$current_block" | python3 -m json.tool 2>&1 | sed 's/^/    /' >&2
        FAIL=1
      fi
      continue
    fi
    if [ "$in_block" -eq 1 ]; then
      current_block="${current_block}${line}"$'\n'
    fi
  done < "$readme"

  if [ "$block_index" -eq 0 ]; then
    echo "  WARN: no JSON blocks found in $readme"
  fi
done

if [ "$FAIL" -ne 0 ]; then
  echo >&2
  echo "FAIL: one or more JSON blocks in MCP example READMEs are invalid." >&2
  exit 1
fi

echo "All MCP example JSON blocks are valid."
