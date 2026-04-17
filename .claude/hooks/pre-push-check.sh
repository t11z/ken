#!/bin/bash
# Blocks git push if cargo fmt, clippy, or tests fail.
# Enforces local CI checks before any code reaches remote.

TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
COMMAND=$(echo "$TOOL_INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('command', ''))" 2>/dev/null)

if [[ "$COMMAND" != git\ push* ]]; then
  exit 0
fi

WORKSPACE_ROOT=$(git rev-parse --show-toplevel 2>/dev/null)
if [ -z "$WORKSPACE_ROOT" ]; then
  exit 0
fi

cd "$WORKSPACE_ROOT" || exit 0

ERRORS=""

FMT_OUTPUT=$(cargo fmt --check 2>&1)
if [ $? -ne 0 ]; then
  ERRORS="cargo fmt --check failed — run 'cargo fmt' to fix.\n${FMT_OUTPUT}"
fi

if [ -z "$ERRORS" ]; then
  CLIPPY_OUTPUT=$(cargo clippy -- -D warnings 2>&1)
  if [ $? -ne 0 ]; then
    ERRORS="cargo clippy -- -D warnings failed.\n${CLIPPY_OUTPUT}"
  fi
fi

if [ -z "$ERRORS" ]; then
  TEST_OUTPUT=$(cargo test --workspace 2>&1)
  if [ $? -ne 0 ]; then
    ERRORS="cargo test --workspace failed.\n${TEST_OUTPUT}"
  fi
fi

if [ -n "$ERRORS" ]; then
  MSG=$(printf '%s' "$ERRORS" | head -c 2000 | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")
  echo "{\"continue\": false, \"stopReason\": \"Pre-push CI check failed: $MSG\"}"
  exit 2
fi

exit 0
