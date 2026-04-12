#!/bin/bash
# Blocks mutation of files that Claude Code may not modify without
# an explicit per-file instruction from the architect.
# Mirrors the "Files Claude Code may not modify" list in CLAUDE.md.

TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
FILE_PATH=$(echo "$TOOL_INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('path', d.get('file_path', '')))" 2>/dev/null)

if [ -z "$FILE_PATH" ]; then
  exit 0
fi

FILE_PATH="${FILE_PATH#./}"
BASENAME=$(basename "$FILE_PATH")

BLOCKED=false
REASON=""

# CLAUDE.md files at any depth
if [[ "$BASENAME" == "CLAUDE.md" ]]; then
  BLOCKED=true
  REASON="CLAUDE.md files are immutable per R5. The architect modifies these directly."
fi

# .claude/ directory (skills, hooks, commands, settings)
if [[ "$FILE_PATH" == .claude/* ]]; then
  BLOCKED=true
  REASON=".claude/ files are immutable per R5. The architect modifies these directly."
fi

# .github/ directory
if [[ "$FILE_PATH" == .github/* ]]; then
  BLOCKED=true
  REASON=".github/ files require explicit architect instruction to modify."
fi

# Specific root files
case "$FILE_PATH" in
  LICENSE|README.md|CONTRIBUTING.md|CODE_OF_CONDUCT.md|Cargo.toml|rust-toolchain.toml)
    BLOCKED=true
    REASON="$FILE_PATH is a protected root file. Modify only when explicitly named in the task."
    ;;
  docs/architecture/repository-structure.md)
    BLOCKED=true
    REASON="repository-structure.md is immutable except by explicit architect instruction."
    ;;
esac

if [ "$BLOCKED" = true ]; then
  echo "{\"block\": true, \"message\": \"Protected files guard: $REASON\"}" >&2
  exit 2
fi

exit 0
