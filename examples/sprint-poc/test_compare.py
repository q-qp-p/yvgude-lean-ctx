import json
import hashlib
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).parent
SDK_ROOT = ROOT.parent.parent / "packages" / "python-lean-ctx"
if str(SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(SDK_ROOT))

from poc import _tamper_canonical_payload, _verify_persisted_receipt, cmd_compare, cmd_verify


FIXTURES = Path(__file__).parent / "tests"


def sealed_receipt_payload() -> dict:
    canonical = {
        "schema_version": "1",
        "receipt_id": "sprint-test-receipt",
        "integration_depth": "wrap",
        "coverage": "observed",
        "execution_receipt_ids": [],
        "integrity_status": "sealed",
        "outcome": "success",
        "degradations": [],
        "savings": {
            "original_tokens": 100,
            "delivered_tokens": 75,
            "saved_tokens": 25,
            "methodology": "compression_observation",
        },
    }
    canonical_json = json.dumps(canonical, sort_keys=True, separators=(",", ":"))
    return {
        "receipt_id": canonical["receipt_id"],
        "canonical_json": canonical_json,
        "canonical_hash": "sha256:" + hashlib.sha256(canonical_json.encode()).hexdigest(),
        "savings": canonical["savings"],
    }


def test_compare_blocks_savings_when_quality_fails(tmp_path):
    runs = tmp_path / "runs"
    stock = runs / "20260821T000000Z-stock"
    treatment = runs / "20260821T000000Z-leanctx"
    stock.mkdir(parents=True)
    treatment.mkdir()
    shutil.copy(FIXTURES / "quality-pass.json", stock / "quality-result.json")
    shutil.copy(FIXTURES / "quality-fail.json", treatment / "quality-result.json")
    (treatment / "execution-receipt.json").write_text("not-json", encoding="utf-8")

    assert cmd_compare(tmp_path) == 2

    comparison = json.loads((tmp_path / "comparison.json").read_text(encoding="utf-8"))
    assert comparison["quality_both_passed"] is False
    assert comparison["savings_claim_allowed"] is False
    assert comparison["savings"] is None


def test_receipt_reverification_rejects_a_tampered_canonical_payload():
    payload = sealed_receipt_payload()

    assert _verify_persisted_receipt(payload) is True
    tampered = _tamper_canonical_payload(payload)
    assert tampered is not None
    assert _verify_persisted_receipt(tampered) is False


def test_compare_requires_a_reverified_treatment_receipt(tmp_path):
    runs = tmp_path / "runs"
    stock = runs / "20260821T000000Z-stock"
    treatment = runs / "20260821T000000Z-leanctx"
    stock.mkdir(parents=True)
    treatment.mkdir()
    shutil.copy(FIXTURES / "quality-pass.json", stock / "quality-result.json")
    shutil.copy(FIXTURES / "quality-pass.json", treatment / "quality-result.json")
    (treatment / "execution-receipt.json").write_text(
        json.dumps(sealed_receipt_payload()),
        encoding="utf-8",
    )

    assert cmd_compare(tmp_path) == 0

    comparison = json.loads((tmp_path / "comparison.json").read_text(encoding="utf-8"))
    assert comparison["treatment_receipt_verified"] is True
    assert comparison["savings_claim_allowed"] is True


def test_verify_rejects_a_written_tampered_receipt(tmp_path):
    treatment = tmp_path / "runs" / "20260821T000000Z-leanctx"
    treatment.mkdir(parents=True)
    (treatment / "execution-receipt.json").write_text(
        json.dumps(sealed_receipt_payload()),
        encoding="utf-8",
    )
    (tmp_path / "comparison.json").write_text(
        json.dumps({"treatment": str(treatment)}),
        encoding="utf-8",
    )

    assert cmd_verify(tmp_path) == 0

    tampered = json.loads(
        (treatment / "execution-receipt.tampered.json").read_text(encoding="utf-8")
    )
    assert _verify_persisted_receipt(tampered) is False
