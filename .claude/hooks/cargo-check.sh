#!/bin/bash
# Runs cargo check on the workspace after any Rust file is edited.
# Gives Claude Code immediate feedback without running the full test suite.

TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
FILE_PATH=$(echo "$TOOL_INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('path', d.get('file_path', '')))" 2>/dev/null)

if [ -z "$FILE_PATH" ]; then
  exit 0
fi

FILE_PATH="${FILE_PATH#./}"

# Only trigger on Rust source files
if [[ "$FILE_PATH" != *.rs ]]; then
  exit 0
fi

# Run from workspace root
WORKSPACE_ROOT=$(git rev-parse --show-toplevel 2>/dev/null)
if [ -z "$WORKSPACE_ROOT" ]; then
  exit 0
fi

cd "$WORKSPACE_ROOT" || exit 0

OUTPUT=$(cargo check --workspace --message-format=short 2>&1)
EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ]; then
  # Non-blocking: report errors but don't block the edit
  echo "{\"feedback\": \"cargo check failed after editing $FILE_PATH:\\n$OUTPUT\"}"
fi

exit 0
