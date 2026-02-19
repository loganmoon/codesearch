#!/bin/bash
# Warn: detect when CWD has drifted out of .worktrees/ after a Bash command.
set -euo pipefail

INPUT=$(cat)

REPO_ROOT="${CLAUDE_PROJECT_DIR:-}"
if [[ -z "$REPO_ROOT" ]]; then
  exit 0
fi

CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
if [[ -z "$CWD" ]]; then
  exit 0
fi

# Only warn if CWD is the repo root (not in a worktree)
if [[ "$CWD" != "$REPO_ROOT" ]]; then
  exit 0
fi

# Check if any worktrees exist (besides main)
WORKTREE_COUNT=$(git -C "$REPO_ROOT" worktree list --porcelain 2>/dev/null | grep -c '^worktree ' || true)
if [[ "$WORKTREE_COUNT" -le 1 ]]; then
  exit 0
fi

# CWD is repo root and worktrees exist -- warn
jq -n '{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "WARNING: CWD is on main but worktrees exist. If you are working in a worktree, cd back into it now."
  }
}'
exit 0
