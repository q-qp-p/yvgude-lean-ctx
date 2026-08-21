"""Deterministic quality gate for the Sprint POC fixture.

Maps reviewer findings onto predeclared defects. Never invents cost or
savings. A missing required defect is a gate failure.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
EXPECTED_PATH = ROOT / "expected-findings.json"


def load_expected(path: Path = EXPECTED_PATH) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def evaluate(review: dict[str, Any], expected: dict[str, Any] | None = None) -> dict[str, Any]:
    spec = expected or load_expected()
    findings = review.get("findings") or []
    if not isinstance(findings, list):
        return {
            "evaluator": spec.get("evaluator"),
            "passed": False,
            "matched": [],
            "missing": [item["id"] for item in spec["required"]],
            "error": "review.findings must be a list",
        }

    matched: list[str] = []
    missing: list[str] = []
    for item in spec["required"]:
        if _matches(findings, item):
            matched.append(item["id"])
        else:
            missing.append(item["id"])

    return {
        "evaluator": spec["evaluator"],
        "passed": not missing,
        "matched": matched,
        "missing": missing,
        "required_count": len(spec["required"]),
        "matched_count": len(matched),
    }


def _matches(findings: list[Any], item: dict[str, Any]) -> bool:
    pattern = re.compile(item["must_match"], re.IGNORECASE)
    for finding in findings:
        if not isinstance(finding, dict):
            continue
        blob = " ".join(
            str(finding.get(key, ""))
            for key in ("id", "location", "summary", "function", "file")
        )
        id_hit = str(finding.get("id", "")).lower() == item["id"]
        loc_hit = item["file"] in blob and item["function"] in blob
        text_hit = pattern.search(blob) is not None
        if id_hit or (loc_hit and text_hit) or (item["file"] in blob and text_hit):
            return True
    return False
