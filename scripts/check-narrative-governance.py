#!/usr/bin/env python3
"""Fail closed when public LeanCTX entry points drift from public claims policy."""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONTRACT = "docs/contracts/public-product-claims-v1.md"
CONTRACT_BLOCK = re.compile(
    r"```json narrative-governance-contract\n(?P<data>.*?)\n```", re.DOTALL
)
ALLOWED_STATUSES = {
    "Available",
    "Experimental",
    "Historical",
    "Local runtime",
    "Preview",
    "Research",
    "Retired",
    "Target",
}


def read(relative_path: str, failures: list[str]) -> str | None:
    path = ROOT / relative_path
    if not path.is_file():
        failures.append(f"required governance file is missing: {relative_path}")
        return None
    return path.read_text(encoding="utf-8")


def is_public_path(relative_path: str) -> bool:
    path = Path(relative_path)
    return (
        not path.is_absolute()
        and ".." not in path.parts
        and not relative_path.startswith("docs/internal/")
    )


def load_contract() -> dict[str, Any]:
    try:
        content = (ROOT / CONTRACT).read_text(encoding="utf-8")
    except FileNotFoundError as error:
        raise ValueError(f"required public claims contract is missing: {CONTRACT}") from error

    match = CONTRACT_BLOCK.search(content)
    if match is None:
        raise ValueError(f"{CONTRACT}: missing narrative-governance JSON block")

    try:
        contract = json.loads(match.group("data"))
    except json.JSONDecodeError as error:
        raise ValueError(f"{CONTRACT}: invalid JSON: {error.msg}") from error

    if not isinstance(contract, dict) or contract.get("schema_version") != 1:
        raise ValueError(f"{CONTRACT}: expected schema_version 1")
    return contract


def validate_contract(contract: dict[str, Any], failures: list[str]) -> None:
    required_text = contract.get("required_text")
    if not isinstance(required_text, dict):
        failures.append(f"{CONTRACT}: required_text must be an object")
    else:
        for relative_path, fragments in required_text.items():
            if not isinstance(relative_path, str) or not is_public_path(relative_path):
                failures.append(
                    f"{CONTRACT}: required_text path must be public: {relative_path!r}"
                )
            if not isinstance(fragments, list) or not all(
                isinstance(fragment, str) and fragment for fragment in fragments
            ):
                failures.append(
                    f"{CONTRACT}: required_text fragments must be non-empty strings: {relative_path!r}"
                )

    status_records = contract.get("status_guarded_records")
    if not isinstance(status_records, list) or not all(
        isinstance(relative_path, str) and is_public_path(relative_path)
        for relative_path in status_records
    ):
        failures.append(f"{CONTRACT}: status_guarded_records must contain public paths")

    feature_statuses = contract.get("feature_statuses")
    if not isinstance(feature_statuses, dict) or not feature_statuses:
        failures.append(f"{CONTRACT}: feature_statuses must be a non-empty object")
    elif not all(
        isinstance(feature, str)
        and feature
        and isinstance(status, str)
        and status in ALLOWED_STATUSES
        for feature, status in feature_statuses.items()
    ):
        failures.append(f"{CONTRACT}: feature_statuses contains an invalid feature or status")


def main() -> int:
    try:
        contract = load_contract()
    except ValueError as error:
        print("Narrative governance failed:", file=sys.stderr)
        print(f"- {error}", file=sys.stderr)
        return 1

    failures: list[str] = []
    validate_contract(contract, failures)

    required_text = contract.get("required_text", {})
    if isinstance(required_text, dict):
        for relative_path, required_fragments in required_text.items():
            if not isinstance(relative_path, str) or not isinstance(required_fragments, list):
                continue
            content = read(relative_path, failures)
            if content is None:
                continue
            for fragment in required_fragments:
                if isinstance(fragment, str) and fragment not in content:
                    failures.append(
                        f"{relative_path}: missing canonical fragment {fragment!r}"
                    )

    status_pattern = re.compile(
        r"(?im)^.{0,3}(?:\*\*)?status(?:\*\*)?\s*:\s*"
        r"(?:available|preview|research|historical|retired|local runtime|local implementation|experimental|target)",
    )
    status_heading_pattern = re.compile(
        r"(?im)^#\s+(?:historical|research|preview|retired|local runtime|target)\b",
    )
    status_records = contract.get("status_guarded_records", [])
    if isinstance(status_records, list):
        for relative_path in status_records:
            if not isinstance(relative_path, str):
                continue
            opening = read(relative_path, failures)
            if opening is None:
                continue
            if not status_pattern.search(opening[:1_500]) and not status_heading_pattern.search(
                opening[:1_500]
            ):
                failures.append(
                    f"{relative_path}: retained or non-current surface needs a prominent status header"
                )

    generated_tools = read("docs/reference/generated/mcp-tools.md", failures)
    if generated_tools is not None:
        if "avoids resending unchanged content" not in generated_tools:
            failures.append(
                "docs/reference/generated/mcp-tools.md: ctx_delta needs its bounded behavior description"
            )
        if "saves 90%+ tokens" in generated_tools:
            failures.append(
                "docs/reference/generated/mcp-tools.md: ctx_delta must not make an unqualified percentage claim"
            )

    if failures:
        print("Narrative governance failed:", file=sys.stderr)
        print("\n".join(f"- {failure}" for failure in failures), file=sys.stderr)
        return 1

    print("Narrative governance passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
