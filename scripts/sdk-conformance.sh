#!/usr/bin/env bash
# Python SDK v1 verification runner.
#
# The canonical SDK root is packages/python-lean-ctx. TypeScript and Go
# prototypes are archived and must not participate in a current release gate.
# This command verifies the published Preview surface without fabricating a
# multi-SDK conformance matrix.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Python SDK v1 tests"
(cd packages/python-lean-ctx && python3 -m pytest -q)

echo "==> Python SDK v1 release metadata"
python3 scripts/check-sdk-versions.py
