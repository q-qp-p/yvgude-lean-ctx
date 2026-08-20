import pytest

from lean_ctx import LeanCTX
from lean_ctx.errors import LeanCtxConnectionError, LeanCtxError


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


def test_kit_name_path_traversal_is_url_escaped(v1_proxy):
    state, base_url = v1_proxy
    LeanCTX({"proxy_url": base_url}).load_kit("../admin/secret")
    assert state.last_request["path"] == "/v1/kits/..%2Fadmin%2Fsecret"
    assert "/admin/secret" not in state.last_request["path"]


def test_kit_missing_identity_fields_raise(v1_proxy):
    state, base_url = v1_proxy
    state.kit_missing_identity = True
    with pytest.raises(LeanCtxError, match="malformed Kit response"):
        LeanCTX({"proxy_url": base_url}).load_kit("payments")


def test_kit_non_200_raises_connection_error(v1_proxy):
    state, base_url = v1_proxy
    state.kit_unavailable = True
    with pytest.raises(LeanCtxConnectionError):
        LeanCTX({"proxy_url": base_url}).load_kit("payments")


def test_load_kit_returns_cached_immutable_handle(v1_proxy):
    _, base_url = v1_proxy
    ctx = LeanCTX({"proxy_url": base_url})
    first = ctx.load_kit("payments")
    second = ctx.load_kit("payments")
    assert first is second
    with pytest.raises(TypeError):
        first.manifest["id"] = "changed"


def test_changed_kit_hash_returns_distinct_handle(v1_proxy):
    state, base_url = v1_proxy
    ctx = LeanCTX({"proxy_url": base_url})
    first = ctx.load_kit("payments")
    state.kit_hash = "c" * 64
    second = ctx.load_kit("payments")
    assert second is not first
    assert first.package_hash != second.package_hash


@pytest.mark.parametrize("name", ["", " ", 42, None])
def test_kit_name_validation_rejects_non_string_or_empty(v1_proxy, name):
    _, base_url = v1_proxy
    with pytest.raises(ValueError, match="Kit name must be a non-empty string"):
        LeanCTX({"proxy_url": base_url}).load_kit(name)
