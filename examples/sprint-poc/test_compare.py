import json
import shutil
from pathlib import Path

from poc import cmd_compare


FIXTURES = Path(__file__).parent / "tests"


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
