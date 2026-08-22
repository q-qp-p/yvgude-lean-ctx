#!/usr/bin/env python3
"""Fail closed when public LeanCTX entry points drift from the internal vision."""

from __future__ import annotations

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
AUTHORITY = "docs/internal/README.md"

# These are the pages contributors are most likely to treat as product truth.
# Keep their required language small, explicit, and coupled to the authority
# rather than trying to ban historical technical terms throughout the repository.
REQUIRED_TEXT: dict[str, tuple[str, ...]] = {
    "README.md": (
        "The Context SDK for AI Agents.",
        "Connect → Measure → Tune → Prove → Deploy → Repeat",
        "It is not an agent platform, a generic agent builder, a hosted execution",
        "Performance Profiles, first-class Context Kits, the Performance",
    ),
    "VISION.md": (
        AUTHORITY,
        "Context SDK for AI Agents",
        "Connect → Measure → Tune → Prove → Deploy → Repeat",
        "first-class Context Kits",
    ),
    "docs/README.md": (
        "Context SDK for AI Agents",
        "internal/README.md",
        "Performance Profiles; first-class Context Kits",
    ),
    "docs/reference/README.md": (
        "Context SDK for existing agents",
        "not a multi-agent platform or orchestration product",
        "No hosted/team/cloud service is publicly available",
    ),
    "docs/guides/README.md": (
        "does not replace the agent or become an agent",
        "Context reduction depends on the file, mode, task, and recovery behavior.",
        "Embed — Preview",
    ),
    "docs/internal/vision/PRODUCT-ARCHITECTURE.md": (
        "The Context SDK for AI Agents",
    ),
    "docs/internal/execution/WEBSITE-REDESIGN.md": (
        "Status:** Internal target copy",
        "Primary navigation:** SDK · Docs · Research",
        "declared quality gate",
    ),
    "docs/internal/vision/01-PRODUCT-ARCHITECTURE.md": (
        "Status: Target architecture",
        "comparable baseline,",
    ),
    "docs/internal/vision/02-OSS-VS-PAID.md": ("Status: Research",),
    "docs/internal/vision/04-GO-TO-MARKET.md": ("Status: Research",),
    "docs/contracts/http-mcp-contract-v1.md": ("Status: Local runtime contract",),
}

# These retained records may discuss superseded systems only when the reader sees
# the status before treating the text as an instruction or availability claim.
STATUS_GUARDED_RECORDS = (
    "clients/rust/lean-ctx-client/README.md",
    "docs/contracts/wrapped-permalink-v1.md",
    "docs/context-os/guide.md",
    "docs/context-os/cookbook-non-coding.md",
    "docs/reference/08-multi-agent.md",
    "docs/reference/09-team-cloud-ci.md",
    "docs/reference/18-adaptive-learning.md",
    "docs/guides/addons.md",
    "docs/guides/hosted-index-slo.md",
    "docs/guides/org-sso-setup.md",
)

CANONICAL_FEATURE_STATUSES = {
    "Performance Profiles": "Research",
    "Context Kits": "Research",
    "Performance Benchmark": "Research",
}


def read(relative_path: str) -> str:
    path = ROOT / relative_path
    if not path.is_file():
        raise RuntimeError(f"required governance file is missing: {relative_path}")
    return path.read_text(encoding="utf-8")


def main() -> int:
    failures: list[str] = []

    for relative_path, required_fragments in REQUIRED_TEXT.items():
        try:
            content = read(relative_path)
        except RuntimeError as error:
            failures.append(str(error))
            continue
        for fragment in required_fragments:
            if fragment not in content:
                failures.append(f"{relative_path}: missing canonical fragment {fragment!r}")

    architecture = read("docs/internal/vision/PRODUCT-ARCHITECTURE.md")
    for feature, status in CANONICAL_FEATURE_STATUSES.items():
        pattern = re.compile(
            rf"(?m)^\|\s+\*\*{re.escape(feature)}\*\*\s+\|.*?\|\s+"
            rf"\*\*{re.escape(status)}\*\*\s+\|"
        )
        if not pattern.search(architecture):
            failures.append(
                f"docs/internal/vision/PRODUCT-ARCHITECTURE.md: {feature} must be {status}"
            )

    status_pattern = re.compile(
        r"(?im)^.{0,3}(?:\*\*)?status(?:\*\*)?\s*:\s*"
        r"(?:available|preview|research|historical|retired|local runtime|target)",
    )
    status_heading_pattern = re.compile(
        r"(?im)^#\s+(?:historical|research|preview|retired|local runtime|target)\b",
    )
    for relative_path in STATUS_GUARDED_RECORDS:
        try:
            opening = read(relative_path)[:1_500]
        except RuntimeError as error:
            failures.append(str(error))
            continue
        if not status_pattern.search(opening) and not status_heading_pattern.search(opening):
            failures.append(
                f"{relative_path}: retained or non-current surface needs a prominent status header"
            )

    if failures:
        print("Narrative governance failed:", file=sys.stderr)
        print("\n".join(f"- {failure}" for failure in failures), file=sys.stderr)
        return 1

    print("Narrative governance passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
