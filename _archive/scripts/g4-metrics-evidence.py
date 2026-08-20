#!/usr/bin/env python3
"""Generate G4 metrics evidence from live lean-ctx self-pilot statistics."""

from __future__ import annotations

import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "security" / "evidence" / "g4-metrics-evidence.json"
STUB_RATIO_TARGET = 35.0
APPEND_STREAM_REDUCTION_TARGET = 35.0


def percentage(original: int, compressed: int) -> float:
    """Return the percentage removed, treating an empty denominator as zero."""
    if original <= 0:
        return 0.0
    return round((original - min(original, compressed)) * 100.0 / original, 2)


def require_int(value: Any, name: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise ValueError(f"stats field {name!r} must be a non-negative integer")
    return value


def load_stats() -> dict[str, Any]:
    try:
        result = subprocess.run(
            ["lean-ctx", "stats", "json"],
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        raise RuntimeError("lean-ctx is not available on PATH") from error
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() or error.stdout.strip() or str(error)
        raise RuntimeError(f"lean-ctx stats json failed: {detail}") from error

    try:
        stats = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("lean-ctx stats json returned invalid JSON") from error
    if not isinstance(stats, dict):
        raise RuntimeError("lean-ctx stats json must return an object")
    return stats


def metrics_from(stats: dict[str, Any]) -> dict[str, Any]:
    cep = stats.get("cep")
    if not isinstance(cep, dict):
        raise ValueError("stats field 'cep' must be an object")
    modes = cep.get("modes")
    if not isinstance(modes, dict):
        raise ValueError("stats field 'cep.modes' must be an object")

    mode_usage = {name: require_int(count, f"cep.modes.{name}") for name, count in modes.items()}
    if not all(isinstance(name, str) for name in mode_usage):
        raise ValueError("cep.modes keys must be strings")

    input_tokens = require_int(stats.get("total_input_tokens"), "total_input_tokens")
    output_tokens = require_int(stats.get("total_output_tokens"), "total_output_tokens")
    cep_original = require_int(cep.get("total_tokens_original"), "cep.total_tokens_original")
    cep_compressed = require_int(cep.get("total_tokens_compressed"), "cep.total_tokens_compressed")
    full_reads = mode_usage.get("full", 0)
    stub_reads = sum(count for mode, count in mode_usage.items() if mode != "full")
    total_reads = full_reads + stub_reads
    stub_ratio = round(stub_reads * 100.0 / total_reads, 2) if total_reads else 0.0
    cep_compression = percentage(cep_original, cep_compressed)
    mode_read_ratio = {
        mode: round(count * 100.0 / total_reads, 2) if total_reads else 0.0
        for mode, count in sorted(mode_usage.items())
    }

    return {
        "total_input_tokens": input_tokens,
        "total_output_tokens": output_tokens,
        "compression_ratio_pct": percentage(input_tokens, output_tokens),
        "stub_reads": stub_reads,
        "full_reads": full_reads,
        "stub_ratio_pct": stub_ratio,
        "cep_sessions": require_int(cep.get("sessions"), "cep.sessions"),
        "cep_original_tokens": cep_original,
        "cep_compressed_tokens": cep_compressed,
        "cep_compression_pct": cep_compression,
        # CEP aggregates the append stream before and after compression, so its
        # measured reduction is the gate's append-stream overhead reduction.
        "append_stream_reduction_pct": cep_compression,
        "mode_usage": dict(sorted(mode_usage.items())),
        "mode_read_ratio_pct": mode_read_ratio,
    }


def main() -> int:
    try:
        metrics = metrics_from(load_stats())
    except (RuntimeError, ValueError) as error:
        print(f"G4 evidence: {error}", file=sys.stderr)
        return 1

    passed = (
        metrics["stub_ratio_pct"] >= STUB_RATIO_TARGET
        and metrics["append_stream_reduction_pct"] >= APPEND_STREAM_REDUCTION_TARGET
    )
    evidence = {
        "gate": "G4",
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "data_source": "lean-ctx stats json (self-pilot)",
        "thresholds": {
            "stub_ratio_pct": STUB_RATIO_TARGET,
            "append_stream_reduction_pct": APPEND_STREAM_REDUCTION_TARGET,
        },
        "metrics": metrics,
        "pass": passed,
    }
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print("G4 Metrics Evidence")
    print(f"  Read modes: {metrics['stub_reads']} non-full / {metrics['full_reads']} full "
          f"({metrics['stub_ratio_pct']:.2f}% non-full; target {STUB_RATIO_TARGET:.0f}%)")
    print(f"  Append stream: {metrics['cep_original_tokens']} -> "
          f"{metrics['cep_compressed_tokens']} tokens "
          f"({metrics['append_stream_reduction_pct']:.2f}% reduction; "
          f"target {APPEND_STREAM_REDUCTION_TARGET:.0f}%)")
    print(f"  Overall: {metrics['total_input_tokens']} -> {metrics['total_output_tokens']} tokens "
          f"({metrics['compression_ratio_pct']:.2f}% reduction)")
    print(f"  Verdict: {'PASS' if passed else 'FAIL'} ({OUTPUT.relative_to(ROOT)})")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
