import pytest

from lean_ctx import LeanCTX
from lean_ctx.errors import LeanCtxError


def test_run_only_preserves_output_and_records_not_addressable(v1_proxy):
    _, base_url = v1_proxy
    output = {"result": "original"}

    class Agent:
        def run(self, task):
            assert task == "Review"
            return output

    run = LeanCTX({"proxy_url": base_url}).wrap(Agent()).run("Review")
    assert run.output is output
    assert run.receipt.coverage == "not_addressable"
    assert "provider_transport_not_bound" in run.receipt.degradations


def test_context_aware_agent_records_bound_response(v1_proxy):
    _, base_url = v1_proxy

    class Agent:
        def run(self, task, *, leanctx):
            assert leanctx.headers["X-LeanCTX-Protocol"] == "1"
            return leanctx.compress([{"role": "user", "content": "long text body"}]).messages

    run = LeanCTX({"proxy_url": base_url}).wrap(Agent()).run("Review")
    assert run.output == [{"role": "user", "content": "long tex"}]
    assert run.receipt.coverage == "compressed"


def test_proxy_bound_reset_runs_after_error(v1_proxy):
    _, base_url = v1_proxy

    class Agent:
        reset = False

        def set_leanctx_transport(self, *, proxy, headers):
            assert proxy is not None
            assert headers["X-LeanCTX-Session-Id"] == "session-v1"

            def reset():
                self.reset = True

            return reset

        def run(self, task):
            raise RuntimeError("agent failure")

    agent = Agent()
    with pytest.raises(RuntimeError, match="agent failure"):
        LeanCTX({"proxy_url": base_url}).wrap(agent).run("Review")
    assert agent.reset is True


def test_fail_open_returns_unsealed_when_completion_transport_fails(v1_proxy):
    state, base_url = v1_proxy
    state.failed_seal = True

    class Agent:
        def run(self, task):
            return {"done": task}

    run = LeanCTX({"proxy_url": base_url, "fail_open": True}).wrap(Agent()).run("Review")
    assert run.output == {"done": "Review"}
    assert run.receipt.integrity_status == "unsealed"
    assert run.receipt.verify() is False


def test_fail_closed_rejects_run_only(v1_proxy):
    _, base_url = v1_proxy

    class Agent:
        def run(self, task):
            return task

    with pytest.raises(LeanCtxError):
        LeanCTX({"proxy_url": base_url, "fail_open": False}).wrap(Agent())
