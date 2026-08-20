import pytest

from lean_ctx import LeanCTX
from lean_ctx.errors import LeanCtxError


def test_kit_response_is_pinned_and_url_quoted(v1_proxy):
    state, base_url = v1_proxy
    kit = LeanCTX({"proxy_url": base_url}).load_kit("payments/core")
    assert kit.id == "kit-payments-core"
    assert state.last_request["path"] == "/v1/kits/payments%2Fcore"
    with pytest.raises(TypeError):
        kit.manifest["id"] = "changed"


def test_invalid_kit_hash_fails_closed(v1_proxy):
    state, base_url = v1_proxy
    state.kit_hash = "uppercase" * 8
    with pytest.raises(LeanCtxError):
        LeanCTX({"proxy_url": base_url}).load_kit("payments")
