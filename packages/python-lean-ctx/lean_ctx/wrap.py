"""The three explicit synchronous v1 agent adapters."""

from __future__ import annotations

import inspect
from dataclasses import dataclass
from typing import TYPE_CHECKING, Mapping, Optional

from .errors import LeanCtxAuthError, LeanCtxConnectionError, LeanCtxError
from .kit import ContextKit
from .profile import TuningProfile
from .proxy import CompressResult, ProxyHTTPResponse, ProxyObservation
from .receipt import ExecutionReceipt
from .session import ContextSession

if TYPE_CHECKING:  # pragma: no cover
    from .core import LeanCTX


@dataclass(frozen=True)
class LeanCtxRun:
    output: object
    receipt: ExecutionReceipt


class RunTransport:
    """Explicit proxy binding supplied only to supported ContextAware agents."""

    def __init__(self, proxy: object, headers: Mapping[str, str], session: ContextSession) -> None:
        self.proxy = proxy
        self.headers = headers
        self._session = session

    def record_proxy_response(self, response: object) -> ProxyObservation:
        return self._session.record_proxy_response(response)

    def compress(self, messages, model=None) -> CompressResult:
        """Bound convenience call that records its observation in call order."""
        result, observation = self.proxy.compress_bound(messages, model, self.headers)
        self.record_proxy_response(observation)
        return result


def _run_signature(agent: object) -> inspect.Signature:
    run = getattr(agent, "run", None)
    if not callable(run):
        raise LeanCtxError("supported agents must define callable run(task)")
    try:
        return inspect.signature(run)
    except (TypeError, ValueError) as exc:
        raise LeanCtxError("agent run signature is not supported by SDK v1") from exc


def _takes_task(signature: inspect.Signature) -> bool:
    parameters = list(signature.parameters.values())
    return any(
        parameter.kind
        in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
        for parameter in parameters
    )


def _has_leanctx_keyword(signature: inspect.Signature) -> bool:
    parameter = signature.parameters.get("leanctx")
    return parameter is not None and parameter.kind in (
        inspect.Parameter.POSITIONAL_OR_KEYWORD,
        inspect.Parameter.KEYWORD_ONLY,
    )


class _Adapter:
    name = ""

    def configure(self, transport: RunTransport):
        return None

    def invoke(self, task: str, transport: RunTransport):
        raise NotImplementedError

    def invoke_unbound(self, task: str):
        raise NotImplementedError


class _ProxyBoundAdapter(_Adapter):
    name = "proxy_bound"

    def __init__(self, agent: object) -> None:
        self.agent = agent

    def configure(self, transport: RunTransport):
        reset = self.agent.set_leanctx_transport(proxy=transport.proxy, headers=transport.headers)
        return reset if callable(reset) else None

    def invoke(self, task: str, transport: RunTransport):
        del transport
        return self.agent.run(task)

    def invoke_unbound(self, task: str):
        return self.agent.run(task)


class _ContextAwareAdapter(_Adapter):
    name = "context_aware"

    def __init__(self, agent: object) -> None:
        self.agent = agent

    def invoke(self, task: str, transport: RunTransport):
        return self.agent.run(task, leanctx=transport)

    def invoke_unbound(self, task: str):
        # No hidden global interception is allowed. ``None`` makes an
        # unavailable lifecycle explicit to an opt-in ContextAware agent.
        return self.agent.run(task, leanctx=None)


class _RunOnlyAdapter(_Adapter):
    name = "run_only"

    def __init__(self, agent: object) -> None:
        self.agent = agent

    def invoke(self, task: str, transport: RunTransport):
        del transport
        return self.agent.run(task)

    def invoke_unbound(self, task: str):
        return self.agent.run(task)


def _select_adapter(agent: object) -> _Adapter:
    signature = _run_signature(agent)
    if not _takes_task(signature):
        raise LeanCtxError("agent run must accept a task positional argument")
    has_transport = callable(getattr(agent, "set_leanctx_transport", None))
    has_context = _has_leanctx_keyword(signature)
    if has_transport and has_context:
        raise LeanCtxError("agent matches both ProxyBoundAgent and ContextAwareAgent")
    if has_transport:
        return _ProxyBoundAdapter(agent)
    if has_context:
        return _ContextAwareAdapter(agent)
    return _RunOnlyAdapter(agent)


class WrappedAgent:
    """A non-reflective wrapper around one supported agent ``run`` method."""

    def __init__(self, ctx: "LeanCTX", agent: object, kit=None, profile=None) -> None:
        if kit is not None and not isinstance(kit, (str, ContextKit)):
            raise ValueError("kit must be a non-empty string, ContextKit, or None")
        if isinstance(kit, str) and not kit.strip():
            raise ValueError("kit must be a non-empty string")
        selected_profile = ctx.config.default_profile if profile is None else profile
        if not isinstance(selected_profile, (str, TuningProfile)):
            raise ValueError("profile must be a non-empty string, TuningProfile, or None")
        if isinstance(selected_profile, str) and not selected_profile.strip():
            raise ValueError("profile must be a non-empty string")
        self._ctx = ctx
        self._agent = agent
        self._adapter = _select_adapter(agent)
        if not ctx.config.fail_open and isinstance(self._adapter, _RunOnlyAdapter):
            raise LeanCtxError("fail_open=False requires a proxy-bound or context-aware agent")
        self._kit = kit
        self._profile = selected_profile
        self._agent_id = ctx._agent_id_for(agent)

    def _degraded_pre_execution_run(self, session: ContextSession, task: str) -> LeanCtxRun:
        session.add_degradation("proxy_session_unavailable")
        output = self._adapter.invoke_unbound(task)
        return LeanCtxRun(output=output, receipt=session.incomplete_receipt())

    def run(self, task) -> LeanCtxRun:
        task_text = ContextSession.validate_task(task)
        session = self._ctx.session()
        session._set_agent_id(self._agent_id)
        try:
            resolved_kit = None if self._kit is None else self._ctx.load_kit(self._kit)
            session._begin(task_text, resolved_kit, self._profile)
        except (LeanCtxConnectionError, LeanCtxAuthError):
            if self._ctx.config.fail_open:
                return self._degraded_pre_execution_run(session, task_text)
            raise

        token = session._bind_current()
        reset = None
        agent_error: Optional[BaseException] = None
        try:
            transport = RunTransport(self._ctx._proxy, session.proxy_headers(), session)
            reset = self._adapter.configure(transport)
            output = self._adapter.invoke(task_text, transport)
            if not session._observations:
                session.add_degradation(
                    "provider_transport_not_bound"
                    if isinstance(self._adapter, _RunOnlyAdapter)
                    else "proxy_response_not_observed"
                )
        except BaseException as exc:
            agent_error = exc
            try:
                session.abort(exc)
            except Exception:
                # The original agent exception remains authoritative.
                pass
            raise
        finally:
            try:
                if reset is not None:
                    reset()
            except Exception:
                if agent_error is None:
                    raise
            finally:
                session._reset_current(token)

        try:
            receipt = session.complete(output)
        except (LeanCtxConnectionError, LeanCtxAuthError):
            if not self._ctx.config.fail_open:
                raise
            session.add_degradation("receipt_sealing_failed")
            receipt = session.incomplete_receipt()
        return LeanCtxRun(output=output, receipt=receipt)
