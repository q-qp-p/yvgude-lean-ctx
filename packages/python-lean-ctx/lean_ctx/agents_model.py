"""OpenAI Agents SDK Model bridge."""

from __future__ import annotations

from contextvars import ContextVar
from dataclasses import dataclass
from typing import Mapping, Optional

from agents.models.interface import Model

from .errors import LeanCtxAuthError, LeanCtxConnectionError, LeanCtxError


@dataclass
class _DirectRun:
    session: object
    transport: object
    token: object


class LeanCtxAgentsModel(Model):
    """Model proxy that compresses input through one bound LeanCTX transport."""

    def __init__(self, delegate: object, owner: "AgentsSdkAdapter") -> None:
        self._delegate = delegate
        self._owner = owner
        self._transport: ContextVar[Optional[object]] = ContextVar(
            "lean_ctx_agents_transport", default=None
        )

    def bind(self, transport: object):
        token = self._transport.set(transport)

        def reset() -> None:
            self._transport.reset(token)

        return reset

    def _delegate_model(self) -> object:
        if not isinstance(self._delegate, (str, type(None))):
            return self._delegate
        from agents.models.multi_provider import MultiProvider

        return MultiProvider().get_model(self._delegate)

    @staticmethod
    def _messages(input: object):
        if isinstance(input, str):
            return [{"role": "user", "content": input}]
        if isinstance(input, list) and all(isinstance(item, Mapping) for item in input):
            return [dict(item) for item in input]
        return None

    def _compress(self, input: object, transport: object):
        messages = self._messages(input)
        if messages is None:
            return input
        return transport.compress(messages).messages

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
        direct_run = None
        transport = self._transport.get()
        if transport is None:
            direct_run = self._owner.start_direct_run(input)
            transport = None if direct_run is None else direct_run.transport
        try:
            compressed_input = input if transport is None else self._compress(input, transport)
            output = await self._delegate_model().get_response(
                system_instructions,
                compressed_input,
                model_settings,
                tools,
                output_schema,
                handoffs,
                tracing,
                previous_response_id=previous_response_id,
                conversation_id=conversation_id,
                prompt=prompt,
            )
        except BaseException as exc:
            if direct_run is not None:
                self._owner.abort_direct_run(direct_run, exc)
            raise
        if direct_run is not None:
            self._owner.complete_direct_run(direct_run, output)
        return output

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
        direct_run = None
        transport = self._transport.get()
        if transport is None:
            direct_run = self._owner.start_direct_run(input)
            transport = None if direct_run is None else direct_run.transport
        try:
            compressed_input = input if transport is None else self._compress(input, transport)
            async for event in self._delegate_model().stream_response(
                system_instructions,
                compressed_input,
                model_settings,
                tools,
                output_schema,
                handoffs,
                tracing,
                previous_response_id=previous_response_id,
                conversation_id=conversation_id,
                prompt=prompt,
            ):
                yield event
        except BaseException as exc:
            if direct_run is not None:
                self._owner.abort_direct_run(direct_run, exc)
            raise
        if direct_run is not None:
            self._owner.complete_direct_run(direct_run, None)

    async def _cleanup_on_run_end(self, owner: object) -> None:
        cleanup = getattr(self._delegate_model(), "_cleanup_on_run_end", None)
        if callable(cleanup):
            await cleanup(owner)

    async def close(self) -> None:
        close = getattr(self._delegate_model(), "close", None)
        if callable(close):
            await close()

    def get_retry_advice(self, request: object):
        advice = getattr(self._delegate_model(), "get_retry_advice", None)
        return None if not callable(advice) else advice(request)


class AgentsSdkAdapter:
    """Clone an Agents SDK Agent with a request-scoped compression Model."""

    def __init__(self, agent: object) -> None:
        self._source_agent = agent
        self._model = LeanCtxAgentsModel(getattr(agent, "model", None), self)
        self.runner_agent = agent.clone(model=self._model)
        self._ctx = None
        self._kit = None
        self._profile = None
        self._agent_id = None
        self.receipt = None
        self.metrics = None

    def set_runtime(self, ctx: object, kit: object, profile: object, agent_id: str) -> None:
        self._ctx = ctx
        self._kit = kit
        self._profile = profile
        self._agent_id = agent_id

    def configure(self, transport: object):
        return self._model.bind(transport)

    def invoke(self, task: str, transport: object):
        del transport
        from agents import Runner

        return Runner.run_sync(self.runner_agent, task)

    def invoke_unbound(self, task: str):
        from agents import Runner

        return Runner.run_sync(self._source_agent, task)

    def _task_text(self, input: object) -> str:
        if isinstance(input, str) and input.strip():
            return input
        if isinstance(input, list):
            for item in input:
                if isinstance(item, Mapping):
                    content = item.get("content")
                    if isinstance(content, str) and content.strip():
                        return content
        return "OpenAI Agents SDK run"

    def start_direct_run(self, input: object) -> Optional[_DirectRun]:
        if self._ctx is None or self._agent_id is None:
            raise LeanCtxError("OpenAI Agents SDK adapter is not configured")
        session = self._ctx.session()
        session._set_agent_id(self._agent_id)
        try:
            kit = None if self._kit is None else self._ctx.load_kit(self._kit)
            session._begin(self._task_text(input), kit, self._profile)
        except (LeanCtxConnectionError, LeanCtxAuthError):
            if not self._ctx.config.fail_open:
                raise
            session.add_degradation("proxy_session_unavailable")
            self.receipt = session.incomplete_receipt()
            return None
        from .wrap import RunTransport

        token = session._bind_current()
        return _DirectRun(session, RunTransport(self._ctx._proxy, session.proxy_headers(), session), token)

    def complete_direct_run(self, direct_run: _DirectRun, output: object) -> None:
        try:
            self.receipt = direct_run.session.complete(output)
        except (LeanCtxConnectionError, LeanCtxAuthError):
            if not self._ctx.config.fail_open:
                raise
            direct_run.session.add_degradation("receipt_sealing_failed")
            self.receipt = direct_run.session.incomplete_receipt()
        finally:
            direct_run.session._reset_current(direct_run.token)

    def abort_direct_run(self, direct_run: _DirectRun, error: BaseException) -> None:
        try:
            self.receipt = direct_run.session.abort(error)
        finally:
            direct_run.session._reset_current(direct_run.token)
