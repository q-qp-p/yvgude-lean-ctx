#!/bin/bash
set -euo pipefail

PROJECT="/Users/yvesgugger/Documents/Privat/Projects/lean-ctx"
GOALS="/tmp/codex-goals-r2"
COMBINED="/tmp/codex-combined-r2"
WTBASE="/tmp/lean-ctx-agents-r2"
TOTAL=10

echo "=== OCLA Agent Orchestration — RUNDE 2 ==="
echo "10 Agents, verbesserte Quality Gates, isolierte Worktrees"
echo ""

cd "$PROJECT"
if [ "$(git branch --show-current)" != "main" ]; then
    echo "ERROR: must be on main"; exit 1
fi

rm -rf "$COMBINED" && mkdir -p "$COMBINED"
for i in $(seq 1 $TOTAL); do
    padded=$(printf "%02d" $i)
    GOAL=$(ls "$GOALS"/agent-${padded}-*.md 2>/dev/null | head -1)
    if [ -z "$GOAL" ]; then
        echo "SKIP agent $padded: no goal file"
        continue
    fi
    cat "$GOALS/preamble.md" "$GOAL" > "$COMBINED/agent-${padded}.md"
done

rm -rf "$WTBASE" && mkdir -p "$WTBASE"
for i in $(seq 1 $TOTAL); do
    padded=$(printf "%02d" $i)
    WT="$WTBASE/agent-$padded"
    BRANCH="r2/agent-$padded"
    git branch -D "$BRANCH" 2>/dev/null || true
    git worktree add "$WT" -b "$BRANCH" main 2>/dev/null
    echo "Worktree: agent-$padded → $BRANCH"
done

echo ""
echo "Launching $TOTAL agents..."
echo ""

for i in $(seq 1 $TOTAL); do
    padded=$(printf "%02d" $i)
    PROMPT_FILE="$COMBINED/agent-${padded}.md"
    WT="$WTBASE/agent-$padded"

    if [ ! -f "$PROMPT_FILE" ]; then
        echo "SKIP agent $padded: no prompt file"
        continue
    fi

    name=$(head -3 "$PROMPT_FILE" | grep -oE 'Agent [0-9]+ — .*' | head -1 || echo "agent-$padded")
    echo "[$i/$TOTAL] $name"

    osascript <<APPLESCRIPT
tell application "Terminal"
    do script "cd $WT && cat $PROMPT_FILE | codex exec -s workspace-write -"
end tell
APPLESCRIPT

    sleep 2
done

echo ""
echo "=== $TOTAL agents launched ==="
echo "Monitor: bash $PROJECT/scripts/monitor-agents-r2.sh"
