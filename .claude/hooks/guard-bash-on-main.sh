#!/bin/bash
# Guard: prevent modifying Bash commands when CWD is on main with worktrees.
# Three layers: worktree escape -> redirect check -> command whitelist.
set -euo pipefail

INPUT=$(cat)
COMMAND=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty')
[[ -z "$COMMAND" ]] && exit 0

REPO_ROOT="${CLAUDE_PROJECT_DIR:-}"
[[ -z "$REPO_ROOT" ]] && exit 0

# Determine CWD: prefer JSON field, fall back to $PWD
CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty')
CWD="${CWD:-$PWD}"

# Only guard when CWD is on main (not inside a worktree)
[[ "$CWD" != "$REPO_ROOT" ]] && exit 0

# Only guard when worktrees exist (more than just main)
if [[ $(git -C "$REPO_ROOT" worktree list 2>/dev/null | wc -l) -le 1 ]]; then
  exit 0
fi

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

# === Layer 1: Worktree escape hatch ===
# Allow if the first action is cd-ing into .worktrees/
first_segment=$(printf '%s' "$COMMAND" | head -1 | sed -E 's/[[:space:]]*(&&|\|\||[;|]).*//')
cd_target=$(printf '%s' "$first_segment" | sed -n 's/^[[:space:]]*cd[[:space:]]\{1,\}//p' | sed 's/^"//;s/"$//' | sed 's/[[:space:]]*$//')
if [[ -n "$cd_target" ]]; then
  if [[ "$cd_target" == .worktrees/* || "$cd_target" == "$REPO_ROOT"/.worktrees/* ]]; then
    exit 0
  fi
fi

# Allow git -C .worktrees/...
git_c_target=$(printf '%s' "$COMMAND" | sed -n 's/^[[:space:]]*git[[:space:]]\{1,\}-C[[:space:]]\{1,\}\([^[:space:]]*\).*/\1/p' | sed 's/^"//;s/"$//')
if [[ -n "$git_c_target" ]]; then
  if [[ "$git_c_target" == .worktrees/* || "$git_c_target" == "$REPO_ROOT"/.worktrees/* ]]; then
    exit 0
  fi
fi

# === Layer 2: Output redirection check ===
# Remove heredocs (<<WORD, <<-WORD) and herestrings (<<<) to avoid false positives
sanitized="$COMMAND"
sanitized=$(printf '%s' "$sanitized" | sed -E "s/<<<[^;|&)]*//g; s/<<-?[[:space:]]*['\"]?[A-Za-z_]+['\"]?//g")
# Remove fd-to-fd redirects (2>&1, >&2, etc.)
sanitized=$(printf '%s' "$sanitized" | sed -E 's/[0-9]*>&[0-9-]+//g')
# Find remaining file redirects: [n]> or [n]>> or &> or &>> followed by a path
redirect_targets=$(printf '%s' "$sanitized" | grep -oE '[0-9&]*>{1,2}[[:space:]]*[^[:space:];&|)>]+' || true)

if [[ -n "$redirect_targets" ]]; then
  while IFS= read -r match; do
    target=$(printf '%s' "$match" | sed -E 's/^[0-9&]*>{1,2}[[:space:]]*//')
    [[ -z "$target" ]] && continue
    case "$target" in
      /dev/null|/dev/stderr|/dev/stdout) continue ;;
      /tmp/*|/tmp) continue ;;
      .worktrees/*) continue ;;
    esac
    if [[ "$target" == "$REPO_ROOT"/.worktrees/* ]]; then
      continue
    fi
    deny "Bash BLOCKED on main: redirect to '$target'. Run in a worktree."
  done <<< "$redirect_targets"
fi

# === Layer 3: Command whitelist ===
declare -A SAFE_CMDS=(
  # Navigation
  [cd]=1 [pwd]=1 [pushd]=1 [popd]=1
  # File reading
  [ls]=1 [cat]=1 [head]=1 [tail]=1 [less]=1 [more]=1 [bat]=1
  [file]=1 [stat]=1 [readlink]=1 [realpath]=1 [wc]=1 [du]=1 [df]=1
  # Text processing (read-only)
  [grep]=1 [egrep]=1 [fgrep]=1 [rg]=1 [ag]=1 [ack]=1
  [sort]=1 [uniq]=1 [cut]=1 [tr]=1 [column]=1 [paste]=1 [join]=1 [comm]=1
  [diff]=1 [cmp]=1 [jq]=1 [yq]=1 [xq]=1 [awk]=1 [gawk]=1 [sed]=1
  # Searching
  [find]=1 [fd]=1 [locate]=1 [which]=1 [type]=1 [whereis]=1 [command]=1
  # Output (safe without redirect; caught by layer 2)
  [echo]=1 [printf]=1 [true]=1 [false]=1 [test]=1 ['[']=1
  # System info
  [uname]=1 [whoami]=1 [id]=1 [hostname]=1 [date]=1 [cal]=1 [time]=1
  [uptime]=1 [free]=1 [env]=1 [printenv]=1 [nproc]=1 [lscpu]=1 [arch]=1
  # Process info
  [ps]=1 [pgrep]=1 [pidof]=1 [lsof]=1 [ss]=1 [netstat]=1
  # Checksums
  [md5sum]=1 [sha256sum]=1 [sha1sum]=1 [shasum]=1 [b2sum]=1
  # Git & GitHub (git subcommands checked separately)
  [git]=1 [gh]=1
  # Infrastructure
  [docker]=1 [docker-compose]=1 [podman]=1 [kubectl]=1 [k9s]=1 [kind]=1 [helm]=1
)

declare -A SAFE_GIT_SUBCMDS=(
  [status]=1 [log]=1 [diff]=1 [show]=1 [blame]=1 [shortlog]=1 [reflog]=1
  [describe]=1 [branch]=1 [tag]=1 [remote]=1 [fetch]=1 [push]=1 [worktree]=1
  [stash]=1 [rev-parse]=1 [ls-files]=1 [cat-file]=1 [ls-tree]=1
  [for-each-ref]=1 [config]=1 [version]=1
)

# Split command on &&, ||, ;, | and newlines; check each segment
while IFS= read -r segment || [[ -n "$segment" ]]; do
  segment=$(printf '%s' "$segment" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
  [[ -z "$segment" ]] && continue

  # Strip leading variable assignments (FOO=bar, FOO="val", FOO='val')
  local_segment="$segment"
  while true; do
    prev="$local_segment"
    local_segment=$(printf '%s' "$local_segment" | sed -E "s/^[A-Za-z_][A-Za-z_0-9]*=(\"[^\"]*\"|'[^']*'|[^[:space:]]*)[[:space:]]+//")
    [[ "$local_segment" == "$prev" ]] && break
  done

  # Bare assignment with no command (e.g., FOO=bar) -- allow
  if printf '%s' "$local_segment" | grep -qE '^[A-Za-z_][A-Za-z_0-9]*='; then
    stripped=$(printf '%s' "$local_segment" | sed -E "s/^[A-Za-z_][A-Za-z_0-9]*=(\"[^\"]*\"|'[^']*'|[^[:space:]]*)$//")
    if [[ -z "$stripped" ]]; then
      continue
    fi
  fi
  [[ -z "$local_segment" ]] && continue

  # Extract first word (the command)
  cmd_word=$(printf '%s' "$local_segment" | awk '{print $1}')
  cmd_base=$(basename "$cmd_word" 2>/dev/null || printf '%s' "$cmd_word")

  # Check whitelist
  if [[ -z "${SAFE_CMDS[$cmd_base]+x}" ]]; then
    deny "Bash BLOCKED on main: '$cmd_base' is not whitelisted. Run in a worktree."
  fi

  # Special: sed -i / --in-place
  if [[ "$cmd_base" == "sed" ]]; then
    sed_args=$(printf '%s' "$local_segment" | sed 's/^[[:space:]]*sed[[:space:]]*//')
    if printf '%s' "$sed_args" | grep -qE '(^|[[:space:]])-[a-zA-Z]*i([[:space:]]|\.|$)|(^|[[:space:]])--in-place([[:space:]]|$)'; then
      deny "Bash BLOCKED on main: 'sed -i/--in-place' modifies files. Run in a worktree."
    fi
  fi

  # Special: git subcommand check
  if [[ "$cmd_base" == "git" ]]; then
    git_subcmd=""
    skip_next=false
    while IFS= read -r word || [[ -n "$word" ]]; do
      if $skip_next; then
        skip_next=false
        continue
      fi
      case "$word" in
        -C|--git-dir|--work-tree|-c) skip_next=true; continue ;;
        -*) continue ;;
        *) git_subcmd="$word"; break ;;
      esac
    done < <(printf '%s' "$local_segment" | awk '{for(i=2;i<=NF;i++) print $i}')

    [[ -z "$git_subcmd" ]] && continue

    if [[ -z "${SAFE_GIT_SUBCMDS[$git_subcmd]+x}" ]]; then
      deny "Bash BLOCKED on main: 'git $git_subcmd' is not whitelisted. Run in a worktree."
    fi

    # Special: only stash list/show are safe
    if [[ "$git_subcmd" == "stash" ]]; then
      stash_action=$(printf '%s' "$local_segment" | sed -n 's/.*stash[[:space:]]\{1,\}\([a-z]*\).*/\1/p')
      case "${stash_action:-push}" in
        list|show) ;;
        *) deny "Bash BLOCKED on main: 'git stash ${stash_action:-push}' is not whitelisted. Run in a worktree." ;;
      esac
    fi
  fi
done < <(printf '%s' "$COMMAND" | sed -E 's/[[:space:]]*(&&|\|\||[;|])[[:space:]]*/\n/g')

# All checks passed
exit 0
