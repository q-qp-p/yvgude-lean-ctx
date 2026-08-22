#!/usr/bin/env python3
"""Render the current canonical SDK status page.

Legacy callers may still pass a scorecard directory and engine version. Those
inputs are intentionally ignored: archived multi-SDK scorecards are not current
product evidence.
"""

from __future__ import annotations

import argparse
import pathlib
import re


ROOT = pathlib.Path(__file__).resolve().parent.parent


def python_version() -> str:
    source = (ROOT / "packages/python-lean-ctx/pyproject.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', source, re.MULTILINE)
    if not match:
        raise SystemExit("canonical Python SDK version not found")
    return match.group(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("matrix_dir", nargs="?", help="ignored legacy scorecard directory")
    parser.add_argument("--engine-version", help="ignored legacy engine version")
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=ROOT / "docs/reference/sdk-conformance-matrix.md",
    )
    args = parser.parse_args()
    version = python_version()
    text = f"""# SDK status and conformance evidence

Current product status is governed by
[`docs/internal/README.md`](../internal/README.md) and the active
[SDK v1 specification](../internal/execution/SDK-V1-SPEC.md).

| Surface | Current status | Canonical location | Claim boundary |
| --- | --- | --- | --- |
| Local Runtime, CLI, MCP, proxy | Available | `rust/` | Local context-performance substrate; not a hosted agent platform. |
| Python SDK v1 | Preview | `packages/python-lean-ctx/` | `lean-ctx-python` {version} with primary import `lean_ctx`; its reference-wrapper and evidence scope are explicit. |
| Rust client | Substrate | `clients/rust/lean-ctx-client/` | A client implementation, not a separate promoted SDK product. |
| TypeScript and Go SDKs | Not current surfaces | `_archive/` | No current distribution, support, or conformance claim. |

Conformance evidence must be generated from a pinned canonical package and a
live Runtime. Do not infer support from archived code, schema fixtures, or a
manual multi-SDK table.
"""
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8")
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
