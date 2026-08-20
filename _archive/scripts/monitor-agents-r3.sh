#!/usr/bin/env bash
set -euo pipefail

WORKTREE_BASE="/tmp/lean-ctx-r3"
REPO="/Users/yvesgugger/Documents/Privat/Projects/lean-ctx"

printf "\n=== OCLA R3 Agent Monitor === %s\n\n" "$(date '+%H:%M:%S')"
printf "%-8s %-8s %-6s %-6s %s\n" "Agent" "Status" "LOC" "Files" "Last Commit"
printf "%-8s %-8s %-6s %-6s %s\n" "------" "------" "-----" "-----" "-----------"

DONE=0
WORK=0
IDLE=0

for i in $(seq -w 1 10); do
  WTDIR="$WORKTREE_BASE/agent-$i"
  if [ ! -d "$WTDIR" ]; then
    printf "%-8s %-8s\n" "$i" "MISSING"
    continue
  fi

  cd "$WTDIR"
  BRANCH="r3/agent-$i"
  DIFF_STAT=$(git diff --stat HEAD~1 2>/dev/null | tail -1)
  COMMIT_MSG=$(git log -1 --format="%s" 2>/dev/null)
  CHANGES=$(git diff --shortstat 2>/dev/null)
  UNTRACKED=$(git ls-files --others --exclude-standard 2>/dev/null | wc -l | tr -d ' ')
  ADDED_LINES=$(git diff --numstat HEAD~1 2>/dev/null | awk '{sum+=$1}END{print sum+0}')
  CHANGED_FILES=$(git diff --name-only HEAD~1 2>/dev/null | wc -l | tr -d ' ')

  HAS_COMMIT=$(git log main.."$BRANCH" --oneline 2>/dev/null | head -1)

  if [ -n "$HAS_COMMIT" ]; then
    STATUS="DONE"
    DONE=$((DONE + 1))
  elif [ -n "$CHANGES" ] || [ "$UNTRACKED" -gt 0 ]; then
    STATUS="WORK"
    WORK=$((WORK + 1))
    ADDED_LINES=$(git diff --numstat 2>/dev/null | awk '{sum+=$1}END{print sum+0}')
    CHANGED_FILES=$(git diff --name-only 2>/dev/null | wc -l | tr -d ' ')
  else
    STATUS="IDLE"
    IDLE=$((IDLE + 1))
    ADDED_LINES="0"
    CHANGED_FILES="0"
  fi

  printf "%-8s %-8s %-6s %-6s %s\n" "$i" "$STATUS" "+$ADDED_LINES" "$CHANGED_FILES" "${COMMIT_MSG:0:60}"
done

cd "$REPO"
echo ""
echo "Summary: $DONE DONE, $WORK WORK, $IDLE IDLE"
echo ""
echo "Quality check (run after all DONE):"
echo "  cd $REPO && cargo test --lib 2>&1 | tail -5"
