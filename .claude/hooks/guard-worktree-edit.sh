#!/bin/bash
# Guard: prevent file edits outside the active worktree.
# Main repo source files are read-only; edits must go through .worktrees/.
set -euo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [[ -z "$FILE_PATH" ]]; then
  exit 0
fi

REPO_ROOT="${CLAUDE_PROJECT_DIR:-}"
if [[ -z "$REPO_ROOT" ]]; then
  exit 0
fi

# Allow: files outside the repo entirely (e.g. ~/.claude/memory)
if [[ "$FILE_PATH" != "$REPO_ROOT"/* ]]; then
  exit 0
fi

# Allow: files inside a worktree
if [[ "$FILE_PATH" == "$REPO_ROOT/.worktrees/"* ]]; then
  exit 0
fi

# Allow: Claude Code config
if [[ "$FILE_PATH" == "$REPO_ROOT/.claude/"* ]]; then
  exit 0
fi

# Deny: everything else under repo root (main is read-only)
jq -n --arg path "$FILE_PATH" '{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": ("File edit BLOCKED: " + $path + " is on main. Edits must target a worktree under .worktrees/. Use `git worktree list` to find the right path.")
  }
}'
