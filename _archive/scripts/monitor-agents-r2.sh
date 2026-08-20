#!/bin/bash
set -euo pipefail

WTBASE="/tmp/lean-ctx-agents-r2"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  OCLA Agent Monitor R2 — $(date '+%H:%M:%S')                           ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

RUNNING=$(ps aux | grep '[c]odex.*exec' | wc -l | tr -d ' ')
echo "Codex-Prozesse aktiv: $RUNNING"
echo ""

printf "%-5s %-6s %-6s %-45s\n" "Agent" "Status" "LOC" "Info"
printf "%-5s %-6s %-6s %-45s\n" "-----" "------" "------" "---------------------------------------------"

DONE=0
WORKING=0
IDLE=0

for i in $(seq -w 1 10); do
    WT="$WTBASE/agent-$i"
    if [ ! -d "$WT" ]; then
        printf "%-5s %-6s %-6s %-45s\n" "$i" "MISS" "-" "worktree missing"
        continue
    fi

    cd "$WT"
    AHEAD=$(git log main..HEAD --oneline 2>/dev/null | wc -l | tr -d ' ')
    CHANGED=$(git diff --name-only 2>/dev/null | wc -l | tr -d ' ')
    ADD=$(git diff --shortstat 2>/dev/null | grep -oE '[0-9]+ insertion' | grep -oE '[0-9]+' || echo "0")
    DEL=$(git diff --shortstat 2>/dev/null | grep -oE '[0-9]+ deletion' | grep -oE '[0-9]+' || echo "0")

    if [ "$AHEAD" -gt 0 ]; then
        STATUS="DONE"
        INFO=$(git log -1 --format='%s' 2>/dev/null | cut -c1-45)
        LOC="+$ADD/-$DEL"
        DONE=$((DONE + 1))
    elif [ "$CHANGED" -gt 0 ]; then
        STATUS="WORK"
        INFO=$(git diff --name-only 2>/dev/null | sed 's|rust/src/||' | tr '\n' ', ' | sed 's/,$//' | cut -c1-45)
        LOC="+$ADD/-$DEL"
        WORKING=$((WORKING + 1))
    else
        STATUS="IDLE"
        INFO="-"
        LOC="-"
        IDLE=$((IDLE + 1))
    fi

    printf "%-5s %-6s %-6s %-45s\n" "$i" "$STATUS" "$LOC" "$INFO"
done

echo ""
echo "Zusammenfassung: $DONE fertig | $WORKING arbeiten | $IDLE idle"
echo ""
echo "Quality Check (nach Merge):"
echo "  cd /Users/yvesgugger/Documents/Privat/Projects/lean-ctx/rust"
echo "  cargo test --lib && cargo clippy --all-features -- -D warnings"
