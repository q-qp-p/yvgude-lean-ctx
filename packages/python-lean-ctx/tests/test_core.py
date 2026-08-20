import pytest

from lean_ctx import ContextKit, LeanCTX, LeanCTXConfig


def test_construction_is_local_and_mapping_normalizes(v1_proxy):
    state, base_url = v1_proxy
    ctx = LeanCTX({"proxy_url": base_url, "proxy_token": "test-token", "timeout": 4})
    assert isinstance(ctx.config, LeanCTXConfig)
    assert ctx.config.timeout == 4
    assert state.requests == []


def test_wrapper_defaults_to_the_local_proxy():
    assert LeanCTX()._proxy.base_url == "http://localhost:8077"


def test_invalid_config_rejected_locally():
    with pytest.raises(ValueError):
        LeanCTX({"unknown": True})
    with pytest.raises(ValueError):
        LeanCTX({"timeout": 0})
    with pytest.raises(ValueError):
        LeanCTX({"integration_depth": "other"})


def test_session_is_pending_and_fresh(v1_proxy):
    _, base_url = v1_proxy
    ctx = LeanCTX({"proxy_url": base_url})
    first, second = ctx.session(), ctx.session()
    assert first is not second
    assert first._phase == second._phase == "pending"


def test_wrap_does_not_run_agent(v1_proxy):
    _, base_url = v1_proxy

    class Agent:
        called = False

        def run(self, task):
            self.called = True
            return task

    agent = Agent()
    LeanCTX({"proxy_url": base_url}).wrap(agent)
    assert agent.called is False


def test_load_kit_caches_verified_identity(v1_proxy):
    state, base_url = v1_proxy
    ctx = LeanCTX({"proxy_url": base_url})
    first = ctx.load_kit("payments")
    second = ctx.load_kit("payments")
    assert first is second
    state.kit_hash = "c" * 64
    third = ctx.load_kit("payments")
    assert third is not first
    assert isinstance(first, ContextKit)
