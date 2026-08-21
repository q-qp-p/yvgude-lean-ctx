import pytest

agents = pytest.importorskip("agents")

from agents import Agent, Runner
from agents.testing import ScriptedModel, assistant_message

from lean_ctx import LeanCTX


def _agent():
    model = ScriptedModel([[assistant_message("approved")]])
    return Agent(name="reviewer", model=model), model


def test_agents_sdk_runner_uses_bound_compression_without_openai_network(v1_proxy):
    state, base_url = v1_proxy
    agent, model = _agent()

    wrapped = LeanCTX({"proxy_url": base_url}).wrap(agent, profile="balanced")
    assert state.requests == []

    result = Runner.run_sync(wrapped, "Review fixture")

    assert result.final_output == "approved"
    assert len(model.calls) == 1
    assert model.first_call.input[0]["content"] == "Review f"
    assert wrapped.receipt is not None
    assert wrapped.receipt.coverage == "compressed"
    assert [request["path"] for request in state.requests].count("/v1/compress") == 1


def test_agents_sdk_wrapper_run_returns_receipt(v1_proxy):
    _, base_url = v1_proxy
    agent, model = _agent()

    run = LeanCTX({"proxy_url": base_url}).wrap(agent).run("Review fixture")

    assert run.output.final_output == "approved"
    assert run.receipt.coverage == "compressed"
    assert len(model.calls) == 1
    assert model.first_call.input[0]["content"] == "Review f"
