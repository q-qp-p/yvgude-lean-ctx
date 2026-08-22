#!/usr/bin/env bash
# Canonical SDK validation entrypoint.
#
# The former cross-language type comparison targeted archived Python,
# TypeScript, and Go prototypes. Current product scope has one Python SDK v1;
# protocol conformance for future bindings must be introduced with an explicit
# support decision and dedicated fixtures.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/check-sdk-versions.py"
