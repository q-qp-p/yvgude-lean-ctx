import os
from types import SimpleNamespace

import pytest

agents = pytest.importorskip("agents")

from agents import Agent, Runner
from agents.items import ModelResponse, ResponseOutputMessage, ResponseOutputText
from agents.models.interface import Model
from agents.usage import Usage

from lean_ctx import LeanCTX


class DeterministicModel(Model):
    def __init__(self):
        self.calls = []

    @property
    def first_call(self):
        return self.calls[0]

    async def get_response(
        self,
        system_instructions,
        input,
        model_settings,
        tools,
        output_schema,
        handoffs,
        tracing,
        *,
        previous_response_id,
        conversation_id,
        prompt,
    ):
        self.calls.append(SimpleNamespace(input=input))
        return ModelResponse(
            output=[
                ResponseOutputMessage(
                    id="local-response",
                    content=[
                        ResponseOutputText(
                            text="approved",
                            type="output_text",
                            annotations=[],
                            logprobs=[],
                        )
                    ],
                    role="assistant",
                    status="completed",
                    type="message",
                )
            ],
            usage=Usage(requests=1),
            response_id="local-response",
        )

    async def stream_response(
        self,
        system_instructions,
        input,
        model_settings,
        tools,
        output_schema,
        handoffs,
        tracing,
        *,
        previous_response_id,
        conversation_id,
        prompt,
    ):
        if False:
            yield None
        raise NotImplementedError("deterministic fixture supports sync runs only")


class FailingModel(DeterministicModel):
    def __init__(self, error):
        super().__init__()
        self.error = error

    async def get_response(self, *args, **kwargs):
        raise self.error


def _agent():
    model = DeterministicModel()
    return Agent(name="reviewer", model=model), model


@pytest.mark.skipif(
    not os.environ.get("LEAN_CTX_ENGINE_BINARY"),
    reason="set LEAN_CTX_ENGINE_BINARY to run the real Engine wrapper proof",
)
def test_agents_sdk_embed_wrapper_uses_real_engine_and_preserves_result(tmp_path, monkeypatch):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(tmp_path / "engine-data"))
    agent, model = _agent()
    session = LeanCTX(
        {
            "engine_binary": os.environ["LEAN_CTX_ENGINE_BINARY"],
            "fail_open": False,
        }
    ).embed("Review", project_root=str(tmp_path))
    view = session.prepare(
        __import__("lean_ctx").ContextSource(str(source), project_root=str(tmp_path))
    )

    result = session.run_openai(agent)

    assert result.final_output == "approved"
    assert model.first_call.input[0]["content"] == view.text
    assert session.receipt.result is result
    assert session.receipt.output == "approved"
    assert session.receipt.outcome == "unknown"
    assert session.receipt.verify() is True


@pytest.mark.skipif(
    not os.environ.get("LEAN_CTX_ENGINE_BINARY"),
    reason="set LEAN_CTX_ENGINE_BINARY to run the real Engine wrapper proof",
)
def test_agents_sdk_embed_wrapper_preserves_exact_exception(tmp_path, monkeypatch):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(tmp_path / "engine-data"))
    error = RuntimeError("provider failed")
    agent = Agent(name="reviewer", model=FailingModel(error))
    session = LeanCTX(
        {
            "engine_binary": os.environ["LEAN_CTX_ENGINE_BINARY"],
            "fail_open": False,
        }
    ).embed("Review", project_root=str(tmp_path))
    session.prepare(
        __import__("lean_ctx").ContextSource(str(source), project_root=str(tmp_path))
    )

    with pytest.raises(RuntimeError) as raised:
        session.run_openai(agent)

    assert raised.value is error
    assert session.receipt.exception is error
    assert session.receipt.outcome == "aborted"
    assert session.receipt.verify() is True


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
