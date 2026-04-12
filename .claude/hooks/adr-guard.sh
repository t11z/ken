#!/bin/bash
# Blocks any mutation to Accepted ADRs in docs/adr/.
# Claude Code may read ADRs freely; it may never modify them.
# Per CLAUDE.md and ADR-0000 immutability rules.

TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
FILE_PATH=$(echo "$TOOL_INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('path', d.get('file_path', '')))" 2>/dev/null)

if [ -z "$FILE_PATH" ]; then
  exit 0
fi

# Normalize: strip leading ./
FILE_PATH="${FILE_PATH#./}"

if [[ "$FILE_PATH" == docs/adr/* ]]; then
  # Check if the file exists and contains "Status: Accepted"
  if [ -f "$FILE_PATH" ] && grep -q "^Status: Accepted" "$FILE_PATH"; then
    echo '{"block": true, "message": "ADR guard: this ADR is Accepted and immutable per ADR-0000. To amend, create a new superseding ADR. Ask the architect if unclear."}' >&2
    exit 2
  fi
  # New ADR files (not yet existing) and Proposed/Superseded ADRs are allowed
fi

exit 0
