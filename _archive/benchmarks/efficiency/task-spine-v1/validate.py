#!/usr/bin/env python3
"""Validate the synthetic Task Spine v1 cohort and print an ETPAO comparison."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
TASKS_DIR = ROOT / "tasks"
TASK_FILES = (
    "task_envelope.json",
    "context_plan.json",
    "execution_receipt.json",
    "baseline.json",
    "outcome.json",
)
EXPECTED_TASKS = 10
SIGNATURE_HEX_LENGTH = 128


class ValidationError(ValueError):
    """A fixture failed a cohort invariant."""


def fail(message: str) -> None:
    raise ValidationError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"{path.relative_to(ROOT)}: invalid JSON: {exc}")
    require(isinstance(value, dict), f"{path.relative_to(ROOT)}: root must be an object")
    return value


def require_fields(value: dict[str, Any], fields: tuple[str, ...], label: str) -> None:
    missing = [field for field in fields if field not in value]
    require(not missing, f"{label}: missing required fields: {', '.join(missing)}")


def require_int(value: Any, field: str, label: str, minimum: int = 0) -> None:
    require(
        isinstance(value, int) and not isinstance(value, bool) and value >= minimum,
        f"{label}: {field} must be an integer >= {minimum}",
    )


def require_string(value: Any, field: str, label: str) -> None:
    require(isinstance(value, str) and bool(value), f"{label}: {field} must be non-empty text")


def validate_envelope(value: dict[str, Any], label: str) -> None:
    required = (
        "schema_version",
        "task_id",
        "trace_id",
        "project_id",
        "session_id",
        "agent_id",
        "complexity",
        "created_at",
    )
    require_fields(value, required, label)
    require(value["schema_version"] == 1, f"{label}: schema_version must be 1")
    for field in required[1:6]:
        require_string(value[field], field, label)
    require(
        value["complexity"] in {"low", "medium", "high", "critical"},
        f"{label}: invalid complexity",
    )
    require_string(value["created_at"], "created_at", label)
    if "risk_class" in value and value["risk_class"] is not None:
        require(value["risk_class"] in {"low", "medium", "high", "critical"}, f"{label}: invalid risk_class")
    if "data_classification" in value and value["data_classification"] is not None:
        require(value["data_classification"] in {"Public", "Internal", "Confidential", "Restricted"}, f"{label}: invalid data_classification")
    if "quality_requirement_milli" in value and value["quality_requirement_milli"] is not None:
        require_int(value["quality_requirement_milli"], "quality_requirement_milli", label)
        require(value["quality_requirement_milli"] <= 1000, f"{label}: quality_requirement_milli > 1000")
    for field in ("cost_budget_micros", "latency_budget_ms"):
        if field in value and value[field] is not None:
            require_int(value[field], field, label)


def validate_context_plan(value: dict[str, Any], task_id: str, label: str) -> None:
    required = (
        "schema_version",
        "task_id",
        "synthetic",
        "data_classification",
        "strategy",
        "budget_tokens",
        "planned_context_tokens",
        "planned_sources",
        "planned_views",
        "expected_tool_calls",
        "rationale",
    )
    require_fields(value, required, label)
    require(value["schema_version"] == 1, f"{label}: schema_version must be 1")
    require(value["task_id"] == task_id, f"{label}: task_id mismatch")
    require(value["synthetic"] is True, f"{label}: fixture must be marked synthetic")
    require(value["data_classification"] == "Public", f"{label}: fixture must be Public")
    require(value["strategy"] in {"minimal", "balanced", "comprehensive", "cached_first"}, f"{label}: invalid strategy")
    for field in ("budget_tokens", "planned_context_tokens", "expected_tool_calls"):
        require_int(value[field], field, label)
    require(isinstance(value["planned_sources"], list) and value["planned_sources"], f"{label}: planned_sources must be non-empty")
    require(isinstance(value["planned_views"], list) and value["planned_views"], f"{label}: planned_views must be non-empty")
    for view in value["planned_views"]:
        require(isinstance(view, dict), f"{label}: each planned view must be an object")
        require_fields(view, ("path", "mode", "reason"), label)
        require_string(view["path"], "planned_views.path", label)
        require_string(view["mode"], "planned_views.mode", label)
        require_string(view["reason"], "planned_views.reason", label)
    require_string(value["rationale"], "rationale", label)


def validate_evidence_ref(value: Any, label: str) -> None:
    require(isinstance(value, dict), f"{label}: evidence reference must be an object")
    required = ("kind", "uri", "digest", "signature_status")
    require_fields(value, required, label)
    require(value["kind"] in {"ProviderReceipt", "RuntimeLog", "SignedBatch", "QualityMeasurement", "ExperimentOutcome"}, f"{label}: invalid evidence kind")
    require_string(value["uri"], "uri", label)
    require_string(value["digest"], "digest", label)
    require(value["digest"].startswith("sha256:"), f"{label}: digest must use sha256: prefix")
    require(value["signature_status"] in {"Verified", "Unverified", "NotSigned"}, f"{label}: invalid signature status")


def validate_receipt(value: dict[str, Any], task_id: str, label: str) -> int:
    required = (
        "schema_version",
        "receipt_id",
        "task_id",
        "plan_id",
        "context_balance",
        "fresh_input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "requested_model",
        "selected_model",
        "provider",
        "model_calls",
        "retries",
        "latency_ms",
        "actual_cost_micros",
        "baseline_cost_micros",
        "avoided_cost_micros",
        "etpao_milli",
        "decision_refs",
        "evidence_refs",
        "signature",
    )
    require_fields(value, required, label)
    require(value["schema_version"] == 1, f"{label}: schema_version must be 1")
    require(value["task_id"] == task_id, f"{label}: task_id mismatch")
    for field in ("receipt_id", "plan_id", "requested_model", "selected_model", "provider", "signature"):
        require_string(value[field], field, label)
    for field in (
        "fresh_input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "model_calls",
        "retries",
        "latency_ms",
        "actual_cost_micros",
        "baseline_cost_micros",
        "avoided_cost_micros",
        "etpao_milli",
    ):
        require_int(value[field], field, label)
    require(value["avoided_cost_micros"] <= value["baseline_cost_micros"], f"{label}: avoided cost exceeds baseline")
    require(len(value["signature"]) == SIGNATURE_HEX_LENGTH, f"{label}: signature must be 64-byte hex")
    try:
        bytes.fromhex(value["signature"])
    except ValueError:
        fail(f"{label}: signature is not hex")
    balance = value["context_balance"]
    require(isinstance(balance, dict), f"{label}: context_balance must be an object")
    require_fields(balance, ("original_tokens", "materialized_tokens", "delivered_tokens", "provider_billed_tokens"), label)
    for field in balance:
        require_int(balance[field], f"context_balance.{field}", label)
    require(balance["materialized_tokens"] <= balance["original_tokens"], f"{label}: materialized exceeds original")
    require(balance["delivered_tokens"] <= balance["materialized_tokens"], f"{label}: delivered exceeds materialized")
    require(isinstance(value["decision_refs"], list), f"{label}: decision_refs must be an array")
    require(isinstance(value["evidence_refs"], list) and value["evidence_refs"], f"{label}: evidence_refs must be non-empty")
    for index, evidence in enumerate(value["evidence_refs"]):
        validate_evidence_ref(evidence, f"{label}: evidence_refs[{index}]")
    return value["fresh_input_tokens"] + value["cached_input_tokens"] + value["output_tokens"] + value["reasoning_tokens"]


def validate_baseline(value: dict[str, Any], task_id: str, label: str) -> int:
    required = (
        "schema_version",
        "task_id",
        "synthetic",
        "data_classification",
        "method",
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "total_tokens",
        "accepted_outcomes",
        "etpao_milli",
        "tool_calls",
        "latency_ms",
        "cost_micros",
    )
    require_fields(value, required, label)
    require(value["schema_version"] == 1, f"{label}: schema_version must be 1")
    require(value["task_id"] == task_id, f"{label}: task_id mismatch")
    require(value["synthetic"] is True and value["data_classification"] == "Public", f"{label}: baseline must be synthetic Public")
    require(value["method"] == "without_optimization", f"{label}: invalid baseline method")
    for field in ("input_tokens", "output_tokens", "reasoning_tokens", "total_tokens", "accepted_outcomes", "etpao_milli", "tool_calls", "latency_ms", "cost_micros"):
        require_int(value[field], field, label)
    require(value["accepted_outcomes"] > 0, f"{label}: accepted_outcomes must be positive")
    expected_total = value["input_tokens"] + value["output_tokens"] + value["reasoning_tokens"]
    require(value["total_tokens"] == expected_total, f"{label}: total_tokens does not equal components")
    require(value["etpao_milli"] == value["total_tokens"] * 1000 // value["accepted_outcomes"], f"{label}: invalid ETPAO")
    return value["total_tokens"]


def validate_outcome(value: dict[str, Any], task_id: str, label: str) -> int:
    required = ("schema_version", "outcome_id", "task_id", "accepted", "quality_score_milli", "signals", "contract_ref", "evidence_refs", "observed_at")
    require_fields(value, required, label)
    require(value["schema_version"] == 1, f"{label}: schema_version must be 1")
    require(value["task_id"] == task_id, f"{label}: task_id mismatch")
    require_string(value["outcome_id"], "outcome_id", label)
    require(value["accepted"] in {"accepted", "rejected", "unknown"}, f"{label}: invalid acceptance state")
    require_int(value["quality_score_milli"], "quality_score_milli", label)
    require(value["quality_score_milli"] <= 1000, f"{label}: quality score > 1000")
    signals = value["signals"]
    signal_names = ("build", "tests", "lint", "typecheck", "completion", "pr", "correction", "rollback", "retry")
    require(isinstance(signals, dict), f"{label}: signals must be an object")
    require_fields(signals, signal_names, label)
    for name in signal_names:
        require(signals[name] in {"passed", "failed", "unknown", "not_run", None}, f"{label}: invalid signal {name}")
    require(isinstance(value["evidence_refs"], list) and value["evidence_refs"], f"{label}: evidence_refs must be non-empty")
    for index, evidence in enumerate(value["evidence_refs"]):
        validate_evidence_ref(evidence, f"{label}: evidence_refs[{index}]")
    require_string(value["observed_at"], "observed_at", label)
    return 1 if value["accepted"] == "accepted" else 0


def validate_task(directory: Path) -> tuple[str, int, int, int]:
    loaded = {name: load_json(directory / name) for name in TASK_FILES}
    label = str(directory.relative_to(ROOT))
    envelope = loaded["task_envelope.json"]
    validate_envelope(envelope, f"{label}/task_envelope.json")
    task_id = envelope["task_id"]
    validate_context_plan(loaded["context_plan.json"], task_id, f"{label}/context_plan.json")
    actual_tokens = validate_receipt(loaded["execution_receipt.json"], task_id, f"{label}/execution_receipt.json")
    baseline_tokens = validate_baseline(loaded["baseline.json"], task_id, f"{label}/baseline.json")
    accepted = validate_outcome(loaded["outcome.json"], task_id, f"{label}/outcome.json")
    require(accepted == 1, f"{label}: fixture must have an accepted outcome")
    actual_etpao = actual_tokens * 1000 // accepted
    require(loaded["execution_receipt.json"]["etpao_milli"] == actual_etpao, f"{label}: receipt ETPAO does not match usage")
    return task_id, baseline_tokens * 1000, actual_etpao, baseline_tokens


def main() -> int:
    directories = sorted(path for path in TASKS_DIR.iterdir() if path.is_dir())
    require(len(directories) == EXPECTED_TASKS, f"expected {EXPECTED_TASKS} task directories, found {len(directories)}")
    rows = [validate_task(directory) for directory in directories]
    ids = [row[0] for row in rows]
    require(len(set(ids)) == EXPECTED_TASKS, "task IDs must be unique across the cohort")
    print("Task Spine v1 — SYNTHETIC PUBLIC ETPAO comparison")
    print("task_id                 baseline    optimized   reduction")
    print("----------------------  ----------  ----------  ---------")
    for task_id, baseline, optimized, _ in rows:
        reduction = (baseline - optimized) / baseline * 100.0
        print(f"{task_id:22}  {baseline:10d}  {optimized:10d}  {reduction:8.1f}%")
    print(f"Validated {len(rows)} task fixtures and {len(rows) * len(TASK_FILES)} JSON artifacts.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValidationError as exc:
        print(f"VALIDATION FAILED: {exc}", file=sys.stderr)
        raise SystemExit(1)
