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

    wrapped = LeanCTX({"proxy_url": base_url}).wrap(Agent())
    run = wrapped.run("Review")
    assert run.output == [{"role": "user", "content": "long tex"}]
    assert run.receipt.coverage == "compressed"
    assert run.metrics.input_tokens == wrapped.input_tokens == 20
    assert run.metrics.output_tokens == wrapped.output_tokens == 5
    assert run.metrics.cached_tokens is None
    assert run.metrics.tool_calls == wrapped.tool_calls == 1
    assert run.metrics.elapsed_ms >= 0
    assert wrapped.elapsed_ms == run.metrics.elapsed_ms


def test_runtime_receipt_preserves_baseline_treatment_cost_comparison(v1_proxy):
    state, base_url = v1_proxy
    state.baseline_cost_micros = 1200
    state.treatment_cost_micros = 450

    class Agent:
        def run(self, task):
            return task

    receipt = LeanCTX({"proxy_url": base_url}).wrap(Agent()).run("Review").receipt
    assert receipt.savings.methodology == "baseline_treatment"
    assert receipt.savings.baseline_cost_micros == 1200
    assert receipt.savings.treatment_cost_micros == 450
    assert receipt.savings.avoided_cost_micros == 750


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

def test_run_only_receives_unchanged_task_argument(v1_proxy):
    _, base_url = v1_proxy
    task = "Review payments"

    class Agent:
        def run(self, received):
            assert received is task
            return received

    run = LeanCTX({"proxy_url": base_url}).wrap(Agent()).run(task)
    assert run.output is task


def test_run_only_receipt_coverage_is_not_addressable(v1_proxy):
    _, base_url = v1_proxy

    class Agent:
        def run(self, task):
            return task

    run = LeanCTX({"proxy_url": base_url}).wrap(Agent()).run("Review")
    assert run.receipt.coverage == "not_addressable"


def test_fail_open_returns_unsealed_when_session_unavailable(v1_proxy):
    state, base_url = v1_proxy
    state.session_unavailable = True

    class Agent:
        def run(self, task):
            return {"done": task}

    run = LeanCTX({"proxy_url": base_url, "fail_open": True}).wrap(Agent()).run("Review")
    assert run.output == {"done": "Review"}
    assert run.receipt.integrity_status == "unsealed"
    assert "proxy_session_unavailable" in run.receipt.degradations
    assert run.receipt.verify() is False


@pytest.mark.parametrize(
    "kwargs",
    [
        {"kit": 123},
        {"kit": ""},
        {"profile": ""},
        {"profile": 42},
    ],
)
def test_wrap_validates_kit_and_profile_arguments(v1_proxy, kwargs):
    _, base_url = v1_proxy

    class Agent:
        def run(self, task):
            return task

    with pytest.raises(ValueError):
        LeanCTX({"proxy_url": base_url}).wrap(Agent(), **kwargs)

