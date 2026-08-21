"""Refuse to leave provider secrets in Sprint POC artifacts."""

from __future__ import annotations

import os
from pathlib import Path

_TEXT_SUFFIXES = {".json", ".txt", ".md", ".log", ".html"}


def leaked_secret_path(root: Path, secret: str | None = None) -> Path | None:
    needle = (secret if secret is not None else os.environ.get("OPENAI_API_KEY", "")).strip()
    if len(needle) < 12:
        return None
    if not root.is_dir():
        return None
    for path in root.rglob("*"):
        if not path.is_file() or path.suffix.lower() not in _TEXT_SUFFIXES:
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if needle in text:
            return path
    return None
