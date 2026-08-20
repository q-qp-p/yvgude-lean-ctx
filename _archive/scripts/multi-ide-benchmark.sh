#!/usr/bin/env bash
# Generate G3 compression evidence with real lean-ctx command output.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
EVIDENCE_DIR="$ROOT/security/evidence"
OUTPUT="$EVIDENCE_DIR/g3-multi-ide-benchmark.json"
WORKSPACE=$(mktemp -d "${TMPDIR:-/tmp}/lean-ctx-g3.XXXXXX")
trap 'rm -rf "$WORKSPACE"' EXIT HUP INT TERM

mkdir -p "$EVIDENCE_DIR"

if [ -x "${LEAN_CTX_BIN:-$HOME/.local/bin/lean-ctx}" ]; then
    LEAN_CTX_BIN=${LEAN_CTX_BIN:-$HOME/.local/bin/lean-ctx}
elif command -v lean-ctx >/dev/null 2>&1; then
    LEAN_CTX_BIN=$(command -v lean-ctx)
else
    printf '%s\n' 'warning: lean-ctx is unavailable; benchmark compression was skipped' >&2
    printf '{\n  "gate": "G3",\n  "generated_at": "%s",\n  "profiles": [],\n  "determinism_check": false,\n  "pass": false,\n  "skipped": "lean-ctx is not available"\n}\n' \
        "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$OUTPUT"
    exit 0
fi

make_python() {
    i=1
    printf '%s\n' '"""Deterministic benchmark fixture."""' > "$WORKSPACE/test.py"
    while [ "$i" -le 199 ]; do
        printf 'def sample_%03d(value): return value + %d\n' "$i" "$i" >> "$WORKSPACE/test.py"
        i=$((i + 1))
    done
}

make_rust() {
    i=1
    : > "$WORKSPACE/test.rs"
    while [ "$i" -le 200 ]; do
        printf 'pub fn sample_%03d(value: usize) -> usize { value + %d }\n' "$i" "$i" >> "$WORKSPACE/test.rs"
        i=$((i + 1))
    done
}

make_json() {
    i=1
    printf '{\n  "records": [\n' > "$WORKSPACE/test.json"
    while [ "$i" -le 100 ]; do
        comma=,
        [ "$i" -eq 100 ] && comma=
        printf '    {"id": %d, "name": "record-%03d", "enabled": true}%s\n' "$i" "$i" "$comma" >> "$WORKSPACE/test.json"
        i=$((i + 1))
    done
    printf '  ]\n}\n' >> "$WORKSPACE/test.json"
}

run_profile() {
    profile=$1
    file=$2
    output=$3
    command="cat '$file'"
    case "$profile" in
        cursor) CURSOR_TASK_ID=benchmark-cursor "$LEAN_CTX_BIN" -c "$command" > "$output" ;;
        claude) CLAUDECODE=benchmark-claude "$LEAN_CTX_BIN" -c "$command" > "$output" ;;
        *) return 2 ;;
    esac
}

json_escape() {
    # Inputs are fixed fixture names and profile labels; retain a portable JSON writer.
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

make_python
make_rust
make_json

timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
tmp_json="$WORKSPACE/profiles.json"
: > "$tmp_json"
first=1
all_ok=true
for profile in cursor claude; do
    for name in test.py test.rs test.json; do
        source="$WORKSPACE/$name"
        compressed="$WORKSPACE/$profile-$name.out"
        run_profile "$profile" "$source" "$compressed"
        original_bytes=$(wc -c < "$source" | tr -d ' ')
        compressed_bytes=$(wc -c < "$compressed" | tr -d ' ')
        [ "$first" -eq 1 ] || printf ',\n' >> "$tmp_json"
        first=0
        printf '    {"ide":"%s","file":"%s","original_bytes":%s,"compressed_bytes":%s}' \
            "$(json_escape "$profile")" "$(json_escape "$name")" "$original_bytes" "$compressed_bytes" >> "$tmp_json"
    done
done

run_profile cursor "$WORKSPACE/test.py" "$WORKSPACE/determinism-a.out"
run_profile cursor "$WORKSPACE/test.py" "$WORKSPACE/determinism-b.out"
if cmp -s "$WORKSPACE/determinism-a.out" "$WORKSPACE/determinism-b.out"; then
    deterministic=true
else
    deterministic=false
    all_ok=false
fi

{
    printf '{\n  "gate": "G3",\n  "generated_at": "%s",\n  "profiles": [\n' "$timestamp"
    cat "$tmp_json"
    printf '\n  ],\n  "determinism_check": %s,\n  "pass": %s\n}\n' "$deterministic" "$all_ok"
} > "$OUTPUT"

printf 'G3 benchmark: %s; deterministic cursor output: %s\n' "$OUTPUT" "$deterministic"
[ "$all_ok" = true ]
