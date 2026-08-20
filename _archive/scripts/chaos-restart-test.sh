#!/usr/bin/env bash
set -euo pipefail

# Chaos Restart Test — G8 Evidence
# Verifies the LaunchAgent KeepAlive mechanism recovers the proxy after a crash.

echo "=== Chaos Restart Test ==="
echo "Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"

proxy_pid() {
    pgrep -f 'lean-ctx.*proxy' | awk 'NR == 1 { print; exit }'
}

echo "[1/5] Checking proxy is running..."
PROXY_PID="$(proxy_pid || true)"
if [ -z "$PROXY_PID" ]; then
    echo "FAIL: Proxy not running. Start with: launchctl load ~/Library/LaunchAgents/com.leanctx.proxy.plist"
    exit 1
fi
echo "  Proxy PID: $PROXY_PID"

echo "[2/5] Checking health endpoint..."
PORT="$(lean-ctx config show 2>/dev/null | awk -F '=' '
    /^[[:space:]]*proxy_port[[:space:]]*=/ {
        value = $2
        gsub(/[^0-9]/, "", value)
        if (value != "") {
            print value
            exit
        }
    }
' || true)"
PORT="${PORT:-4444}"
HEALTH="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PORT}/health" 2>/dev/null || echo 000)"
echo "  Health: HTTP $HEALTH"
if [ "$HEALTH" != "200" ]; then
    echo "FAIL: Proxy health check failed before chaos test. Health: $HEALTH"
    exit 1
fi

echo "[3/5] Killing proxy (simulating crash)..."
kill -9 "$PROXY_PID"
echo "  Killed PID $PROXY_PID"

echo "[4/5] Waiting for auto-restart (max 10s)..."
NEW_PID=""
i=1
while [ "$i" -le 20 ]; do
    sleep 0.5
    CANDIDATE_PID="$(proxy_pid || true)"
    if [ -n "$CANDIDATE_PID" ] && [ "$CANDIDATE_PID" != "$PROXY_PID" ]; then
        NEW_PID="$CANDIDATE_PID"
        echo "  Restarted! New PID: $NEW_PID (after ${i}x0.5s)"
        break
    fi
    i=$((i + 1))
done

echo "[5/5] Verifying health after restart..."
sleep 1
NEW_HEALTH="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PORT}/health" 2>/dev/null || echo 000)"

EVIDENCE_DIR="security/evidence"
mkdir -p "$EVIDENCE_DIR"
PASS=false
if [ -n "$NEW_PID" ] && [ "$NEW_HEALTH" = "200" ]; then
    PASS=true
fi

cat > "${EVIDENCE_DIR}/g8-chaos-restart-evidence.json" <<EOF
{
  "gate": "G8",
  "test": "chaos-restart",
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "original_pid": $PROXY_PID,
  "new_pid": ${NEW_PID:-0},
  "health_before": "$HEALTH",
  "health_after": "$NEW_HEALTH",
  "restart_mechanism": "LaunchAgent KeepAlive",
  "pass": $PASS
}
EOF

if [ "$PASS" = true ]; then
    echo
    echo "PASS: Proxy auto-restarted after kill. G8 chaos test passed."
    exit 0
fi

echo
echo "FAIL: Proxy did not recover. Health: $NEW_HEALTH"
exit 1
