from pathlib import Path

from secret_guard import leaked_secret_path


def test_leaked_secret_path_finds_key(tmp_path: Path):
    secret = "sk-test-not-a-real-provider-key"
    (tmp_path / "execution-receipt.json").write_text(
        '{"note": "contains ' + secret + '"}\n', encoding="utf-8"
    )
    assert leaked_secret_path(tmp_path, secret) == tmp_path / "execution-receipt.json"


def test_leaked_secret_path_clean(tmp_path: Path):
    (tmp_path / "quality-result.json").write_text('{"passed": true}\n', encoding="utf-8")
    assert leaked_secret_path(tmp_path, "sk-test-not-a-real-provider-key") is None
