#!/usr/bin/env python3
"""Verify metadata for the one canonical Python SDK v1 surface."""

from __future__ import annotations

import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = ROOT / "packages/python-lean-ctx"


def value(source: str, key: str) -> str:
    match = re.search(rf'^{re.escape(key)}\s*=\s*"([^"]+)"', source, re.MULTILINE)
    if not match:
        raise ValueError(f"missing {key!r}")
    return match.group(1)


def main() -> int:
    pyproject = PACKAGE / "pyproject.toml"
    source = pyproject.read_text(encoding="utf-8")
    try:
        name = value(source, "name")
        version = value(source, "version")
    except ValueError as error:
        print(f"SDK metadata error: {error}", file=sys.stderr)
        return 1

    if name != "lean-ctx-python":
        print(f"SDK metadata error: expected lean-ctx-python, found {name!r}", file=sys.stderr)
        return 1
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", version):
        print(f"SDK metadata error: invalid semantic version {version!r}", file=sys.stderr)
        return 1

    required = ("core.py", "session.py", "wrap.py", "receipt.py")
    missing = [name for name in required if not (PACKAGE / "lean_ctx" / name).is_file()]
    if missing:
        print(f"SDK layout error: missing canonical modules: {', '.join(missing)}", file=sys.stderr)
        return 1

    print(f"OK: canonical Python SDK lean-ctx-python {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
