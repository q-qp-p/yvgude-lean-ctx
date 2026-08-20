#!/usr/bin/env bash
set -euo pipefail

PREAMBLE="/tmp/codex-goals-r3/preamble.md"
GOALS_DIR="/tmp/codex-goals-r3"
COMBINED_DIR="/tmp/codex-combined-r3"
WORKTREE_BASE="/tmp/lean-ctx-r3"

GOALS=(
  "agent-01-model-router-registry"
  "agent-02-config-tuner-callsite"
  "agent-03-connector-scheduler-callsite"
  "agent-04-experiment-runner-callsite"
  "agent-05-agent-gateway-registry"
  "agent-06-intent-classifier-real"
  "agent-07-efficiency-analyzer-bugfix"
  "agent-08-thin-adapter-fixes"
  "agent-09-unified-ledger-schema"
  "agent-10-docs-sync"
)

mkdir -p "$COMBINED_DIR"

for i in $(seq 0 9); do
  NUM=$(printf "%02d" $((i + 1)))
  GOAL_FILE="$GOALS_DIR/${GOALS[$i]}.md"
  COMBINED="$COMBINED_DIR/agent-$NUM.md"
  WTDIR="$WORKTREE_BASE/agent-$NUM"

  if [ ! -f "$GOAL_FILE" ]; then
    echo "SKIP agent-$NUM: $GOAL_FILE not found"
    continue
  fi
  if [ ! -d "$WTDIR" ]; then
    echo "SKIP agent-$NUM: worktree $WTDIR not found"
    continue
  fi

  cat "$PREAMBLE" > "$COMBINED"
  echo "" >> "$COMBINED"
  echo "---" >> "$COMBINED"
  echo "" >> "$COMBINED"
  cat "$GOAL_FILE" >> "$COMBINED"

  echo "Starting agent-$NUM in $WTDIR ..."
  osascript -e "
    tell application \"Terminal\"
      do script \"cd $WTDIR && cat $COMBINED | codex exec -s workspace-write - 2>&1 | tee /tmp/codex-r3-agent-$NUM.log\"
    end tell
  "
  sleep 2
done

echo ""
echo "=== Alle 10 Agents gestartet ==="
echo "Monitor: bash scripts/monitor-agents-r3.sh"
