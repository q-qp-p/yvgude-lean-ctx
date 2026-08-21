sdk-conformance.sh 86L cognitive
// /Users/yvesgugger/Documents/Privat/Projects/lean-ctx/scripts/sdk-conformance.sh
§ block block (L1-L39)
#!/usr/bin/env bash
# SDK conformance matrix runner (GL #395).
#
# Builds the engine (unless LEAN_CTX_BIN points at one), starts a real
# `lean-ctx serve` on an ephemeral port, then runs the conformance kit of all
# four first-party SDKs against it:
#
#   * Python   packages/python-lean-ctx   (pytest  tests/test_conformance_live.py)
#   * TypeScript cookbook/sdk            (vitest  src/conformance.e2e.test.ts)
#   * Go       go-sdk                    (go test TestConformanceLive)
#   * Rust     clients/rust/lean-ctx-client (cargo test --test conformance_live)
#
# Each suite writes its scorecard into $LEANCTX_MATRIX_DIR; afterwards the
# matrix generator renders docs/reference/sdk-conformance-matrix.md.
#
# Usage:  scripts/sdk-conformance.sh [--keep-server]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN="${LEAN_CTX_BIN:-$ROOT/rust/target/debug/lean-ctx}"
if [[ ! -x "$BIN" ]]; then
  echo "==> building engine (debug, all features)"
  (cd rust && cargo build --all-features)
fi
[[ -x "$BIN" ]] || { echo "engine binary missing: $BIN"; exit 1; }

PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
URL="http://127.0.0.1:$PORT"
TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
MATRIX_DIR="$(mktemp -d)"
export LEANCTX_CONFORMANCE_URL="$URL"
export LEANCTX_CONFORMANCE_TOKEN="$TOKEN"
export LEANCTX_MATRIX_DIR="$MATRIX_DIR"

echo "==> starting lean-ctx serve on $URL"
"$BIN" serve --host 127.0.0.1 --port "$PORT" --project-root "$ROOT" --auth-token "$TOKEN" &
SERVER_PID=$!
§ function function (L40-L44)
cleanup() {
  if [[ "${1:-}" != "--keep-server" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
  fi
}
2/3 chunks shown (498 tokens)
[lean-ctx] full source: read "/Users/yvesgugger/Documents/Privat/Projects/lean-ctx/scripts/sdk-conformance.sh" directly (no MCP)  ·  or ctx_read("/Users/yvesgugger/Documents/Privat/Projects/lean-ctx/scripts/sdk-conformance.sh", mode="full")
