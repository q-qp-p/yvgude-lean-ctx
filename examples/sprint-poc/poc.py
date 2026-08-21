#!/usr/bin/env python3
"""Sprint POC harness: doctor, stock/treatment run, compare, verify.

Does not invent savings. A claim requires both quality gates and a
re-verifiable persisted receipt.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from agent import ReferenceCodeReviewAgent  # noqa: E402
from quality import evaluate, load_expected  # noqa: E402
from secret_guard import leaked_secret_path  # noqa: E402

MANIFEST = json.loads((ROOT / "workload-manifest.json").read_text(encoding="utf-8"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="poc.py")
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("doctor")
    run = sub.add_parser("run")
    run.add_argument("--arm", choices=("stock", "leanctx"), required=True)
    run.add_argument("--out", type=Path, required=True)
    cmp_p = sub.add_parser("compare")
    cmp_p.add_argument("--out", type=Path, required=True)
    ver = sub.add_parser("verify")
    ver.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.cmd == "doctor":
        return cmd_doctor()
    if args.cmd == "run":
        return cmd_run(args.arm, args.out)
    if args.cmd == "compare":
        return cmd_compare(args.out)
    return cmd_verify(args.out)


def cmd_doctor() -> int:
    checks: list[tuple[str, bool, str, bool]] = []
    checks.append(("python", True, sys.version.split()[0], True))
    checks.append(("fixture", (ROOT / "fixture" / "checkout.py").is_file(), "checkout.py", True))
    checks.append(("expected-findings", (ROOT / "expected-findings.json").is_file(), "json", True))
    checks.append(("kit", (REPO / "kits" / "code-review" / "kit.toml").is_file(), "kits/code-review", True))
    key = bool(os.environ.get("OPENAI_API_KEY", "").strip())
    checks.append(("OPENAI_API_KEY", key, "set" if key else "missing — required only for run", False))
    proxy = _proxy_reachable()
    checks.append(("lean-ctx proxy", proxy, "loopback" if proxy else "not reachable", False))
    sdk_path = REPO / "packages" / "python-lean-ctx"
    try:
        import lean_ctx  # noqa: F401

        checks.append(("lean-ctx-python", True, "import ok", True))
    except ImportError:
        checks.append(
            (
                "lean-ctx-python",
                False,
                f"missing — set PYTHONPATH={sdk_path} or pip install -e {sdk_path}",
                True,
            )
        )

    print("LeanCTX Sprint POC preflight")
    failed = 0
    for name, ok, detail, required in checks:
        mark = "ok" if ok else "FAIL" if required else "WARN"
        print(f"  {mark:4}  {name}: {detail}")
        failed += int(required and not ok)
    print("READY" if failed == 0 else "NOT READY")
    return 0 if failed == 0 else 1


def cmd_run(arm: str, out_root: Path) -> int:
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + f"-{arm}"
    run_dir = out_root / "runs" / run_id
    run_dir.mkdir(parents=True, exist_ok=False)
    shutil.copy(ROOT / "workload-manifest.json", run_dir / "workload-manifest.json")

    agent = ReferenceCodeReviewAgent()
    task = load_expected()["task"]
    receipt_payload: dict[str, Any] | None = None
    try:
        if arm == "stock":
            review = agent.run(task, leanctx=None)
        else:
            review, receipt_payload = _run_wrapped(agent, task)
    except Exception as exc:
        (run_dir / "error.txt").write_text(str(exc), encoding="utf-8")
        print(f"error: {exc}")
        return 1

    quality = evaluate(review)
    (run_dir / f"{arm}-output.json").write_text(
        json.dumps(review, indent=2) + "\n", encoding="utf-8"
    )
    (run_dir / "quality-result.json").write_text(
        json.dumps(quality, indent=2) + "\n", encoding="utf-8"
    )
    if receipt_payload is not None:
        (run_dir / "execution-receipt.json").write_text(
            json.dumps(receipt_payload, indent=2) + "\n", encoding="utf-8"
        )

    leaked = leaked_secret_path(run_dir)
    if leaked is not None:
        print(f"error: provider secret leaked into {leaked}")
        return 1
    print(f"RUN {arm}/{run_id}")
    print(f"  Quality: {'PASS' if quality['passed'] else 'FAIL'} "
          f"({quality['matched_count']}/{quality['required_count']})")
    if quality["missing"]:
        print(f"  Missing: {', '.join(quality['missing'])}")
    print(f"  Output:  {run_dir}")
    return 0 if quality["passed"] else 2


def _run_wrapped(agent: ReferenceCodeReviewAgent, task: str) -> tuple[dict[str, Any], dict[str, Any]]:
    from lean_ctx import LeanCTX

    ctx = LeanCTX(
        {
            "project": "sprint-poc",
            "default_profile": "balanced",
            "fail_open": False,
        }
    )
    wrapped = ctx.wrap(agent, kit="code-review", profile="balanced")
    run = wrapped.run(task)
    review = run.output
    if not isinstance(review, dict):
        raise TypeError("wrapped agent must return ReviewResult dict")
    receipt = run.receipt
    canonical_json = getattr(receipt, "_canonical_json", b"")
    if isinstance(canonical_json, bytes):
        canonical_json = canonical_json.decode("utf-8", errors="strict")
    if not isinstance(canonical_json, str):
        canonical_json = ""
    payload = {
        "receipt_id": getattr(receipt, "receipt_id", None),
        "canonical_json": canonical_json,
        "canonical_hash": getattr(receipt, "canonical_hash", None),
        "signature": getattr(receipt, "signature", None),
        "signer_public_key": getattr(receipt, "_public_key", None),
        "savings": _savings_dict(receipt),
        "degradations": list(getattr(receipt, "degradations", ()) or ()),
        "coverage": getattr(receipt, "coverage", None),
        "integrity_status": getattr(receipt, "integrity_status", None),
    }
    payload["verified"] = _verify_persisted_receipt(payload)
    return review, payload


def _savings_dict(receipt: Any) -> dict[str, Any] | None:
    savings = getattr(receipt, "savings", None)
    if savings is None:
        return None
    return {
        "original_tokens": getattr(savings, "original_tokens", None),
        "delivered_tokens": getattr(savings, "delivered_tokens", None),
        "saved_tokens": getattr(savings, "saved_tokens", None),
        "methodology": getattr(savings, "methodology", None),
        "provider_input_tokens": getattr(savings, "provider_input_tokens", None),
        "provider_output_tokens": getattr(savings, "provider_output_tokens", None),
        "baseline_cost_micros": getattr(savings, "baseline_cost_micros", None),
        "treatment_cost_micros": getattr(savings, "treatment_cost_micros", None),
        "quality_status": getattr(savings, "quality_status", None),
    }


def cmd_compare(out_root: Path) -> int:
    runs = sorted((out_root / "runs").glob("*"))
    stock = _latest(runs, "stock")
    treatment = _latest(runs, "leanctx")
    if stock is None or treatment is None:
        print("error: need one stock run and one leanctx run")
        return 1
    stock_q = json.loads((stock / "quality-result.json").read_text(encoding="utf-8"))
    treat_q = json.loads((treatment / "quality-result.json").read_text(encoding="utf-8"))
    both_pass = bool(stock_q.get("passed") and treat_q.get("passed"))
    receipt_path = treatment / "execution-receipt.json"
    receipt_verified = False
    savings = None
    if receipt_path.is_file():
        try:
            receipt_payload = json.loads(receipt_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            receipt_payload = None
        if isinstance(receipt_payload, dict):
            receipt_verified = _verify_persisted_receipt(receipt_payload)
            if receipt_verified:
                savings = receipt_payload.get("savings")
    savings_claim_allowed = both_pass and receipt_verified
    if not both_pass:
        note = "Quality gate failed on at least one arm. No savings claim."
    elif not receipt_verified:
        note = "Treatment receipt is missing, malformed, or failed re-verification. No savings claim."
    else:
        note = None
    comparison = {
        "baseline": str(stock),
        "treatment": str(treatment),
        "quality_both_passed": both_pass,
        "treatment_receipt_verified": receipt_verified,
        "savings_claim_allowed": savings_claim_allowed,
        "savings": savings if savings_claim_allowed else None,
        "note": note,
    }
    dest = out_root / "comparison.json"
    dest.write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(comparison, indent=2))
    return 0 if savings_claim_allowed else 2


def cmd_verify(out_root: Path) -> int:
    comparison_path = out_root / "comparison.json"
    if not comparison_path.is_file():
        print("error: run compare first")
        return 1
    comparison = json.loads(comparison_path.read_text(encoding="utf-8"))
    receipt = Path(comparison["treatment"]) / "execution-receipt.json"
    if not receipt.is_file():
        print("error: treatment receipt missing")
        return 1
    try:
        payload = json.loads(receipt.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        print("error: treatment receipt is malformed")
        return 2
    if not isinstance(payload, dict):
        print("error: treatment receipt must be a JSON object")
        return 2
    verified = _verify_persisted_receipt(payload)
    print(f"treatment receipt verified: {verified}")
    tampered = _tamper_canonical_payload(payload)
    if tampered is None:
        print("error: treatment receipt has no canonical payload to tamper-check")
        return 2
    copy = receipt.with_name("execution-receipt.tampered.json")
    copy.write_text(json.dumps(tampered, indent=2) + "\n", encoding="utf-8")
    tampered_rejected = not _verify_persisted_receipt(tampered)
    print(f"wrote tampered copy: {copy}")
    print(f"tampered receipt rejected: {tampered_rejected}")
    return 0 if verified and tampered_rejected else 2


def _verify_persisted_receipt(payload: dict[str, Any]) -> bool:
    """Re-parse persisted receipt material; never trust a stored boolean."""
    receipt_id = payload.get("receipt_id")
    if not isinstance(receipt_id, str) or not receipt_id:
        return False
    try:
        from lean_ctx.receipt import parse_execution_receipt

        receipt = parse_execution_receipt(payload)
    except Exception:
        return False
    return receipt.receipt_id == receipt_id and receipt.verify()


def _tamper_canonical_payload(payload: dict[str, Any]) -> dict[str, Any] | None:
    canonical_json = payload.get("canonical_json")
    if not isinstance(canonical_json, str):
        return None
    try:
        canonical = json.loads(canonical_json)
    except json.JSONDecodeError:
        return None
    if not isinstance(canonical, dict):
        return None
    canonical["outcome"] = "tampered"
    tampered = dict(payload)
    tampered["canonical_json"] = json.dumps(
        canonical,
        sort_keys=True,
        separators=(",", ":"),
    )
    return tampered


def _latest(runs: list[Path], arm: str) -> Path | None:
    matches = [path for path in runs if path.name.endswith(f"-{arm}")]
    return matches[-1] if matches else None


def _proxy_reachable() -> bool:
    try:
        result = subprocess.run(
            ["lean-ctx", "status"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


if __name__ == "__main__":
    raise SystemExit(main())
