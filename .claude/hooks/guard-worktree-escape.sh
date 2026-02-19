#!/bin/bash
# Guard: prevent leaving a worktree without permission.
# When CWD is inside a worktree and a command would cd to the repo root,
# check git/GitHub state to decide whether to allow or block.
#
# ALLOW if the branch's PR is merged (cleanup is expected).
# DENY  if there are uncommitted changes, unpushed commits, an open PR,
#       or no PR exists yet.
set -euo pipefail

INPUT=$(cat)
COMMAND=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty')
[[ -z "$COMMAND" ]] && exit 0

REPO_ROOT="${CLAUDE_PROJECT_DIR:-}"
[[ -z "$REPO_ROOT" ]] && exit 0

# Determine CWD
CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty')
CWD="${CWD:-$PWD}"

# Only guard when CWD is inside a worktree
[[ "$CWD" != "$REPO_ROOT"/.worktrees/* ]] && exit 0

# Extract worktree root (e.g., /repo/.worktrees/329--foo/client -> /repo/.worktrees/329--foo)
WORKTREE_ROOT=$(printf '%s' "$CWD" | sed -E "s|^($REPO_ROOT/\.worktrees/[^/]+).*|\1|")

deny() {
  jq -n --arg r "$1" '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "deny",
      "permissionDecisionReason": $r
    }
  }'
  exit 0
}

# === Detect cd-to-repo-root ===
has_escape=false
has_worktree_reentry=false

while IFS= read -r segment || [[ -n "$segment" ]]; do
  segment=$(printf '%s' "$segment" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
  [[ -z "$segment" ]] && continue

  printf '%s' "$segment" | grep -qE '^\s*cd\s' || continue

  cd_target=$(printf '%s' "$segment" | sed -n 's/^[[:space:]]*cd[[:space:]]\{1,\}//p' | sed 's/^"//;s/"$//' | sed 's/[[:space:]]*$//')
  [[ -z "$cd_target" ]] && continue

  resolved=$(cd "$CWD" && realpath -m "$cd_target" 2>/dev/null || printf '%s' "$cd_target")

  if [[ "$resolved" == "$REPO_ROOT" ]]; then
    has_escape=true
  elif [[ "$resolved" == "$REPO_ROOT"/.worktrees/* ]]; then
    has_worktree_reentry=true
  fi
done < <(printf '%s' "$COMMAND" | sed -E 's/[[:space:]]*(&&|\|\||[;|])[[:space:]]*/\n/g')

# No escape detected
$has_escape || exit 0

# If command also re-enters a worktree (switching worktrees), allow
$has_worktree_reentry && exit 0

# === State checks (cheapest first) ===
BRANCH=$(git -C "$WORKTREE_ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
[[ -z "$BRANCH" || "$BRANCH" == "HEAD" ]] && exit 0

# 1. Uncommitted changes (cheapest)
if [[ -n $(git -C "$WORKTREE_ROOT" status --porcelain 2>/dev/null | head -1) ]]; then
  deny "Leaving worktree BLOCKED: uncommitted changes on '$BRANCH'. Commit or stash before leaving."
fi

# 2. Unpushed commits (cheap, but need upstream)
HAS_UPSTREAM=$(git -C "$WORKTREE_ROOT" rev-parse --abbrev-ref '@{u}' 2>/dev/null || echo "")
if [[ -z "$HAS_UPSTREAM" ]]; then
  deny "Leaving worktree BLOCKED: branch '$BRANCH' has never been pushed."
fi
if [[ -n $(git -C "$WORKTREE_ROOT" log '@{u}..HEAD' --oneline 2>/dev/null | head -1) ]]; then
  deny "Leaving worktree BLOCKED: unpushed commits on '$BRANCH'. Push before leaving."
fi

# 3. PR state (expensive -- only reached when tree is clean and pushed)
REMOTE_URL=$(git -C "$WORKTREE_ROOT" remote get-url origin 2>/dev/null || echo "")
REPO_SLUG=$(printf '%s' "$REMOTE_URL" | sed -E 's|.*github\.com[:/]||;s|\.git$||')
PR_STATE=$(gh pr view "$BRANCH" --repo "$REPO_SLUG" --json state -q .state 2>/dev/null || echo "NONE")

if [[ "$PR_STATE" == "MERGED" ]]; then
  exit 0
fi

if [[ "$PR_STATE" == "OPEN" ]]; then
  deny "Leaving worktree BLOCKED: PR for '$BRANCH' is still open."
fi

deny "Leaving worktree BLOCKED: no merged PR found for '$BRANCH'. Stay in the worktree or get user approval."
