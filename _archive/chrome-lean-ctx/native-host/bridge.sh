#!/bin/bash
# Minimal debug: write to /tmp to verify Chrome calls this script
echo "CALLED $(date) PID=$$ HOME=$HOME" > /tmp/lean-ctx-bridge.log
echo "PATH=$PATH" >> /tmp/lean-ctx-bridge.log
echo "0=$0" >> /tmp/lean-ctx-bridge.log

# Read input and respond with Python (one-shot)
SCRIPT_DIR="$(cd "$(dirname "$0")" 2>/dev/null && pwd)"
/usr/bin/python3 -u "$SCRIPT_DIR/bridge.py" 2>>/tmp/lean-ctx-bridge-err.log

echo "EXIT=$?" >> /tmp/lean-ctx-bridge.log
