import pytest

from lean_ctx import LeanCTX
from lean_ctx.errors import LeanCtxError


def _session(base_url):
    ctx = LeanCTX({"proxy_url": base_url})
    session = ctx.session()
    session._set_agent_id("agent-v1")
    return ctx, session


def test_pending_session_does_not_contact_runtime(v1_proxy):
    state, base_url = v1_proxy
    _, session = _session(base_url)
    assert session._phase == "pending"
    assert state.requests == []


@pytest.mark.parametrize("task", ["", " ", 42, "x" * (16 * 1024 + 1)])
def test_begin_rejects_invalid_tasks(v1_proxy, task):
    _, base_url = v1_proxy
    _, session = _session(base_url)
    with pytest.raises(ValueError):
        session._begin(task, None, "balanced")


def test_begin_sends_selectors_and_proxy_headers(v1_proxy):
    state, base_url = v1_proxy
    _, session = _session(base_url)
    session._begin("Review payments", None, "balanced")
    assert state.last_request["body"]["task"] == "Review payments"
    assert state.last_request["body"]["requested_profile"] == "balanced"
    headers = session.proxy_headers()
    assert set(headers) == {
        "X-LeanCTX-Protocol",
        "X-LeanCTX-Session-Id",
        "X-LeanCTX-Agent-Id",
        "X-LeanCTX-Task-Id",
        "X-LeanCTX-Trace-Id",
        "X-LeanCTX-Event-Id",
    }
    assert "Review payments" not in headers.values()


def test_bound_observation_preserves_unknown_as_none(v1_proxy):
    _, base_url = v1_proxy
    ctx, session = _session(base_url)
    session._begin("Observe", None, "balanced")
    _, observation = ctx._proxy.compress_bound(
        [{"role": "user", "content": "a long request"}], None, session.proxy_headers()
    )
    session.record_proxy_response(observation)
    assert observation.usage["cached_tokens"] is None
    assert observation.usage["reasoning_tokens"] is None


def test_invalid_known_observation_value_is_protocol_error(v1_proxy):
    state, base_url = v1_proxy
    state.invalid_header = True
    ctx, session = _session(base_url)
    session._begin("Observe", None, "balanced")
    with pytest.raises(LeanCtxError):
        ctx._proxy.compress_bound([], None, session.proxy_headers())
