"""Task-scoped synchronous session handle for the v1 Runtime protocol."""

from __future__ import annotations

import hashlib
import json
import uuid
import urllib.parse
from types import MappingProxyType
from typing import TYPE_CHECKING, Dict, Mapping, Optional, Tuple

from .engine import (
    ENGINE_INTERFACE_VERSION,
    ContextPlan,
    ContextSource,
    ContextView,
    LocalEngineClient,
    PREVIEW_VERSION,
)
from .errors import (
    LeanCtxEngineError,
    LeanCtxEngineExecutionError,
    LeanCtxEngineProtocolError,
    LeanCtxEngineRejected,
    LeanCtxEngineTimeout,
    LeanCtxEngineUnavailable,
    LeanCtxError,
)
from .kit import ContextKit, parse_kit
from .profile import TuningProfile, parse_profile
from .proxy import ProxyHTTPResponse, ProxyObservation, parse_proxy_observation
from .receipt import (
    ContextReceipt,
    ExecutionReceipt,
    make_unsealed_receipt,
    parse_execution_receipt,
)

if TYPE_CHECKING:  # pragma: no cover
    from .core import LeanCTX

_MAX_TASK_BYTES = 16 * 1024
_OPAQUE_ALLOWED = set(chr(number) for number in range(33, 127))


def _opaque(value: object, name: str) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > 256
        or any(character not in _OPAQUE_ALLOWED for character in value)
    ):
        raise LeanCtxError(f"invalid Runtime {name}")
    return value


def _task(task: object) -> str:
    if not isinstance(task, str) or not task.strip():
        raise ValueError("task must be a non-empty string")
    if len(task.encode("utf-8")) > _MAX_TASK_BYTES:
        raise ValueError("task must not exceed 16 KiB")
    return task


def _profile_selector(profile: object) -> object:
    if isinstance(profile, TuningProfile):
        return {
            "id": profile.id,
            "version": profile.version,
            "content_hash": profile.content_hash,
        }
    if not isinstance(profile, str) or not profile.strip():
        raise ValueError("profile must be a non-empty string or TuningProfile")
    return profile


def _kit_selector(kit: ContextKit) -> Dict[str, str]:
    return {
        "id": kit.id,
        "version": kit.version,
        "package_hash": kit.package_hash,
        "activation_ref": kit.activation_ref,
    }


def _observation_payload(observation: ProxyObservation) -> Dict[str, object]:
    return {
        "request_id": observation.request_id,
        "execution_receipt_id": observation.execution_receipt_id,
        "canonical_hash": observation.canonical_hash,
        "usage": dict(observation.usage),
        "coverage": observation.coverage,
        "provider": observation.provider,
        "model": observation.model,
        "latency_ms": observation.latency_ms,
    }


class ContextSession:
    """Client handle for exactly one root task execution."""

    def __init__(
        self,
        ctx: "LeanCTX",
        task: Optional[str] = None,
        *,
        integration_depth: Optional[str] = None,
        project_root: Optional[str] = None,
        fail_open: Optional[bool] = None,
    ) -> None:
        self._ctx = ctx
        self._phase = "pending"
        if integration_depth is not None and integration_depth not in {"wrap", "embed"}:
            raise ValueError("integration_depth must be wrap or embed for a ContextSession")
        self._local = (
            integration_depth == "embed"
            or task is not None
            or (integration_depth is None and ctx.config.integration_depth == "embed")
        )
        self._local_state = "created" if self._local else "pending"
        self._local_fail_open = ctx.config.fail_open if fail_open is None else fail_open
        if not isinstance(self._local_fail_open, bool):
            raise ValueError("fail_open must be a boolean")
        self._local_project_root = project_root
        self._local_engine = (
            LocalEngineClient(binary=ctx.config.engine_binary, timeout=ctx.config.engine_timeout)
            if self._local
            else None
        )
        self._session_id: Optional[str] = None
        self._task_id: Optional[str] = None
        self._run_id: Optional[str] = None
        self._trace_id: Optional[str] = None
        self._task: Optional[str] = None
        self._profile: Optional[TuningProfile] = None
        self._kits: Tuple[ContextKit, ...] = ()
        self._observations: list[ProxyObservation] = []
        self._degradations: list[str] = []
        self._receipt: Optional[ExecutionReceipt] = None
        self._agent_id: Optional[str] = ctx.config.agent_id
        self._project_id: Optional[str] = None
        self._lineage_event_id: Optional[str] = None
        self._headers: Optional[Mapping[str, str]] = None
        self._terminal_event_ids: Dict[str, str] = {}
        self._local_task: Optional[str] = None
        self._local_plan: Optional[ContextPlan] = None
        self._local_view: Optional[ContextView] = None
        self._local_receipt: Optional[ContextReceipt] = None
        if self._local:
            if integration_depth not in (None, "embed"):
                raise ValueError("local Preview sessions require integration_depth='embed'")
            self._local_task = None if task is None else _task(task)
            self._session_id = "preview-session-" + uuid.uuid4().hex
            self._task_id = "preview-task-" + uuid.uuid4().hex
            self._run_id = "preview-run-" + uuid.uuid4().hex
            self._trace_id = "preview-trace-" + uuid.uuid4().hex
            if self._local_task is not None:
                self._local_state = "created"

    @staticmethod
    def validate_task(task: object) -> str:
        return _task(task)

    @property
    def state(self) -> str:
        """Public Preview lifecycle state; legacy sessions retain old phases."""
        return self._local_state if self._local else self._phase

    @property
    def status(self) -> str:
        """Alias for :attr:`state` used by host integrations."""
        return self.state

    @property
    def integration_depth(self) -> str:
        return "embed" if self._local else "wrap"

    @property
    def phase(self) -> str:
        return self._phase

    @property
    def session_id(self) -> Optional[str]:
        return self._session_id

    @property
    def task_id(self) -> Optional[str]:
        return self._task_id

    @property
    def run_id(self) -> Optional[str]:
        return self._run_id

    @property
    def trace_id(self) -> Optional[str]:
        return self._trace_id

    @property
    def task(self) -> Optional[str]:
        return self._local_task if self._local else self._task

    @property
    def plan_record(self) -> Optional[ContextPlan]:
        return self._local_plan

    @property
    def plan_id(self) -> Optional[str]:
        return None if self._local_plan is None else self._local_plan.plan_id

    @property
    def view(self) -> Optional[ContextView]:
        return self._local_view

    @property
    def receipt(self):
        return self._local_receipt if self._local else self._receipt

    def begin(self, task: Optional[str] = None) -> "ContextSession":
        """Set the host task for a Preview session without contacting Engine."""
        if not self._local:
            raise LeanCtxError("begin() is only available for integration_depth='embed'")
        if self._local_state not in {"created", "prepared"}:
            raise LeanCtxError("Preview session is already terminal")
        if task is not None:
            task_text = _task(task)
            if self._local_task is not None and self._local_task != task_text:
                raise LeanCtxError("Preview session task cannot be changed")
            self._local_task = task_text
        if self._local_task is None:
            raise ValueError("task must be a non-empty string")
        self._local_state = "created"
        self._phase = "created"
        return self

    def plan(
        self,
        source: ContextSource,
        *,
        mode: str = "aggressive",
        budget_tokens: Optional[int] = None,
    ) -> ContextPlan:
        """Create one deterministic explicit source plan; no Engine planning."""
        if not self._local:
            raise LeanCtxError("plan() is only available for integration_depth='embed'")
        self.begin()
        if self._local_state not in {"created", "prepared"}:
            raise LeanCtxError("cannot plan a terminal Preview session")
        if not isinstance(source, ContextSource):
            raise TypeError("source must be a ContextSource")
        if self._local_project_root is not None:
            source = ContextSource(
                source.path,
                project_root=self._local_project_root,
                media_type=source.media_type,
                source_ref=source.source_ref,
                source_digest=source.source_digest,
            )
        if mode != "aggressive":
            raise ValueError("Preview Engine v1 requires mode='aggressive'")
        if budget_tokens is not None:
            if (
                isinstance(budget_tokens, bool)
                or not isinstance(budget_tokens, int)
                or budget_tokens <= 0
                or budget_tokens > 16_000_000
            ):
                raise ValueError("budget_tokens must be a bounded positive integer")
        source_descriptor = {
            "path": source.relative_path,
            "media_type": source.media_type,
            "mode": mode,
            "budget_tokens": budget_tokens,
        }
        if source.source_ref is not None:
            source_descriptor["source_ref"] = source.source_ref
        if source.source_digest is not None:
            source_descriptor["source_digest"] = source.source_digest
        canonical = json.dumps(
            {
                "preview_version": PREVIEW_VERSION,
                "engine_interface_version": ENGINE_INTERFACE_VERSION,
                "task": self._local_task,
                "source": source_descriptor,
            },
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
        ).encode("utf-8")
        plan_id = "preview-plan-" + hashlib.sha256(canonical).hexdigest()[:32]
        self._local_plan = ContextPlan(
            session=self,
            task_id=self._task_id or "",
            source=source,
            plan_id=plan_id,
            mode=mode,
            budget_tokens=budget_tokens,
        )
        self._local_state = "prepared"
        self._phase = "prepared"
        return self._local_plan

    def prepare(
        self,
        source: Optional[ContextSource] = None,
        *,
        mode: str = "aggressive",
        budget_tokens: Optional[int] = None,
    ) -> Optional[ContextView]:
        """Execute the explicit plan through Engine and return its bounded view."""
        if not self._local:
            raise LeanCtxError("prepare() is only available for integration_depth='embed'")
        try:
            plan = self._local_plan if source is None else self.plan(
                source, mode=mode, budget_tokens=budget_tokens
            )
            if plan is None:
                raise ValueError("prepare() requires a ContextSource or prior plan")
            return plan.execute()
        except LeanCtxEngineRejected as exc:
            if self._local_fail_open and exc.view is not None:
                self._local_view = exc.view
                self._local_state = "executing"
                self._phase = "executing"
                self._local_degradations_add("engine_policy_rejected")
                return exc.view
            self._abort_local(exc)
            raise
        except LeanCtxEngineExecutionError as exc:
            if self._local_fail_open and exc.view is not None:
                self._local_view = exc.view
                self._local_state = "executing"
                self._phase = "executing"
                code = exc.view.failure.code if exc.view.failure is not None else "failed"
                self._local_degradations_add("engine_" + code)
                return exc.view
            self._abort_local(exc)
            raise
        except (LeanCtxEngineUnavailable, LeanCtxEngineTimeout) as exc:
            if self._local_fail_open:
                self._local_state = "executing"
                self._phase = "executing"
                self._local_degradations_add(
                    "engine_timeout" if isinstance(exc, LeanCtxEngineTimeout) else "engine_unavailable"
                )
                return None
            self._abort_local(exc)
            raise
        except LeanCtxEngineProtocolError as exc:
            if self._local_fail_open and "has no receipt link" in str(exc):
                self._local_state = "executing"
                self._phase = "executing"
                self._local_degradations_add("engine_receipt_unavailable")
                return None
            self._abort_local(exc)
            raise
        except LeanCtxEngineError as exc:
            # Malformed lineage, digests, versions, or receipt links are never
            # silently converted into Engine evidence.
            self._abort_local(exc)
            raise

    def _execute_local_plan(self, plan: ContextPlan) -> ContextView:
        if not self._local:
            raise LeanCtxError("local Preview plan is not bound to this session")
        if self._local_plan is not plan:
            raise LeanCtxError("plan is not the session's explicit plan")
        if self._local_state not in {"prepared", "executing"}:
            raise LeanCtxError("Preview session is not prepared for Engine execution")
        if self._local_view is not None:
            if self._local_view.status == "rejected":
                raise LeanCtxEngineRejected("Engine policy rejected the explicit source", view=self._local_view)
            if self._local_view.status == "failed":
                code = self._local_view.failure.code if self._local_view.failure is not None else "unknown"
                raise LeanCtxEngineExecutionError("Engine operation failed: " + code, view=self._local_view)
            return self._local_view
        self._local_state = "executing"
        self._phase = "executing"
        engine = self._local_engine
        if engine is None:  # pragma: no cover - local sessions always create one
            raise LeanCtxEngineUnavailable("local Engine transport is unavailable")
        try:
            view = engine.context_view(plan)
        except LeanCtxEngineRejected:
            raise
        except LeanCtxEngineExecutionError:
            raise
        self._local_view = view
        if view.status == "rejected":
            raise LeanCtxEngineRejected("Engine policy rejected the explicit source", view=view)
        if view.status == "failed":
            code = view.failure.code if view.failure is not None else "unknown"
            raise LeanCtxEngineExecutionError("Engine operation failed: " + code, view=view)
        if view.status == "degraded":
            self._local_degradations_add("engine_degraded")
        return view

    def _local_degradations_add(self, value: str) -> None:
        if not hasattr(self, "_local_degradations"):
            self._local_degradations: list[str] = []
        if value not in self._local_degradations:
            self._local_degradations.append(value)

    def run_openai(self, agent: object, *, input: object = None, outcome: Optional[str] = None):
        """Run one maintained OpenAI Agents SDK path against the prepared view.

        The original RunResult is returned unchanged. Engine preparation,
        completion, abort, and receipt projection remain explicit Python
        lifecycle operations; the framework is neither patched nor wrapped
        globally.
        """
        if not self._local:
            raise LeanCtxError("run_openai() requires integration_depth='embed'")
        from .agents import is_agents_sdk_agent

        if not is_agents_sdk_agent(agent):
            raise LeanCtxError("run_openai() requires an OpenAI Agents SDK Agent")
        if self._local_state == "prepared":
            self.prepare()
        if self._local_state != "executing":
            raise LeanCtxError("Preview session is not ready for agent execution")
        model_input = input
        if model_input is None:
            model_input = (
                self._local_view.text
                if self._local_view is not None and self._local_view.text is not None
                else self._local_task
            )
        try:
            from agents import Runner

            result = Runner.run_sync(agent, model_input)
        except BaseException as exc:
            self._abort_local(exc)
            raise
        self.complete(
            result,
            outcome=outcome,
            host_output=getattr(result, "final_output", None),
            tool_results=getattr(result, "new_items", None),
        )
        return result

    def _abort_local(self, error: BaseException) -> Optional[ContextReceipt]:
        if self._local_receipt is not None:
            self._local_state = "aborted"
            return self._local_receipt
        self._local_receipt = ContextReceipt.from_session(
            session_id=self._session_id,
            task_id=self._task_id,
            run_id=self._run_id,
            trace_id=self._trace_id,
            plan=self._local_plan,
            view=self._local_view,
            outcome="aborted",
            host_exception=error,
            degradations=tuple(getattr(self, "_local_degradations", ())),
        )
        self._local_state = "aborted"
        self._phase = "aborted"
        return self._local_receipt

    def _set_agent_id(self, agent_id: str) -> None:
        if self._phase != "pending":
            raise LeanCtxError("cannot change agent identity after session creation")
        self._agent_id = _opaque(agent_id, "agent_id")

    def _begin(self, task: object, kit: object, profile: object) -> None:
        """Create and configure the Runtime session immediately before agent work."""
        task_text = _task(task)
        if self._phase != "pending":
            raise LeanCtxError("session has already begun")
        if kit is None:
            kits: Tuple[ContextKit, ...] = ()
        elif isinstance(kit, ContextKit):
            kits = (kit,)
        elif isinstance(kit, tuple) and all(isinstance(item, ContextKit) for item in kit):
            kits = kit
        else:
            raise LeanCtxError("session Kit must be a resolved ContextKit")
        if self._agent_id is None:
            raise LeanCtxError("agent identity was not assigned")
        requested_profile = _profile_selector(profile)
        payload = {
            "task": task_text,
            "project": self._ctx.config.project,
            "agent_id": self._agent_id,
            "requested_profile": requested_profile,
            "requested_kits": [_kit_selector(item) for item in kits],
            "integration_depth": "wrap",
            "protocol_version": "1",
        }
        data, _ = self._ctx._proxy._post_response("/v1/sessions", payload)
        reply: Dict[str, object] = dict(data)
        nested = data.get("session")
        if isinstance(nested, Mapping):
            reply.update(nested)
        try:
            session_id = _opaque(reply.get("session_id"), "session_id")
            task_id = _opaque(reply.get("task_id"), "task_id")
            run_id = _opaque(reply.get("run_id"), "run_id")
            trace_id = _opaque(reply.get("trace_id"), "trace_id")
            response_agent_id = reply.get("agent_id", self._agent_id)
            if _opaque(response_agent_id, "agent_id") != self._agent_id:
                raise LeanCtxError("Runtime agent identity does not match requested identity")
            response_profile = reply.get("resolved_profile", reply.get("profile"))
            resolved_profile = parse_profile(response_profile)
            response_kits = reply.get("resolved_kits", reply.get("kits"))
            if not isinstance(response_kits, (list, tuple)):
                raise LeanCtxError("Runtime did not return resolved Kit pins")
            resolved_kits = tuple(parse_kit(item) for item in response_kits)
        except (LeanCtxError, ValueError) as exc:
            raise LeanCtxError(f"invalid session create response: {exc}") from exc

        if isinstance(profile, TuningProfile) and (
            (resolved_profile.id, resolved_profile.version, resolved_profile.content_hash)
            != (profile.id, profile.version, profile.content_hash)
        ):
            raise LeanCtxError("Runtime profile pin does not match requested pin")
        requested_kits = {(item.id, item.version, item.package_hash) for item in kits}
        received_kits = {(item.id, item.version, item.package_hash) for item in resolved_kits}
        if requested_kits != received_kits:
            raise LeanCtxError("Runtime Kit pins do not match requested pins")

        # The state becomes visible only after the complete acknowledgement has
        # passed its identity and pin validation.
        self._session_id = session_id
        self._task_id = task_id
        self._run_id = run_id
        self._trace_id = trace_id
        self._task = task_text
        self._profile = resolved_profile
        self._kits = resolved_kits
        project_id = reply.get("project_id")
        self._project_id = None if project_id is None else _opaque(project_id, "project_id")
        self._lineage_event_id = uuid.uuid4().hex
        self._headers = MappingProxyType(
            {
                "X-LeanCTX-Protocol": "1",
                "X-LeanCTX-Session-Id": self._session_id,
                "X-LeanCTX-Agent-Id": self._agent_id,
                "X-LeanCTX-Task-Id": self._task_id,
                "X-LeanCTX-Trace-Id": self._trace_id,
                "X-LeanCTX-Event-Id": self._lineage_event_id,
            }
        )
        self._phase = "executing"

    def proxy_headers(self) -> Mapping[str, str]:
        if self._phase != "executing" or self._headers is None:
            raise LeanCtxError("session is not executing")
        return self._headers

    def _bind_current(self) -> object:
        if self._phase != "executing":
            raise LeanCtxError("session is not executing")
        return self._ctx._current_session.set(self)

    def _reset_current(self, token: object) -> None:
        self._ctx._current_session.reset(token)

    def record_proxy_response(self, response: object) -> ProxyObservation:
        if self._phase != "executing":
            raise LeanCtxError("cannot record a proxy response outside execution")
        if isinstance(response, ProxyObservation):
            observation = response
        elif isinstance(response, ProxyHTTPResponse):
            observation = parse_proxy_observation(response)
        else:
            raise LeanCtxError("proxy response must be ProxyHTTPResponse or ProxyObservation")
        self._observations.append(observation)
        return observation

    def add_degradation(self, degradation: str) -> None:
        if not isinstance(degradation, str) or not degradation:
            raise ValueError("degradation must be a non-empty string")
        if degradation not in self._degradations:
            self._degradations.append(degradation)

    def _coverage(self) -> str:
        if not self._observations:
            return "not_addressable"
        if all(item.coverage == "not_addressable" for item in self._observations):
            return "not_addressable"
        return self._observations[-1].coverage

    @staticmethod
    def _output_digest(output: object) -> str:
        try:
            encoded = json.dumps(
                output,
                sort_keys=True,
                separators=(",", ":"),
                ensure_ascii=False,
                default=repr,
            ).encode("utf-8")
        except (TypeError, ValueError):  # pragma: no cover - defensive object fallback
            encoded = repr(output).encode("utf-8", "replace")
        return "sha256:" + hashlib.sha256(encoded).hexdigest()

    def _terminal_event_id(self, action: str) -> str:
        event_id = self._terminal_event_ids.get(action)
        if event_id is None:
            event_id = uuid.uuid4().hex
            self._terminal_event_ids[action] = event_id
        return event_id

    def _terminal_payload(self, output: object = None, error: object = None) -> Dict[str, object]:
        payload: Dict[str, object] = {
            "event_id": self._terminal_event_id("complete" if error is None else "abort"),
            "observations": [_observation_payload(item) for item in self._observations],
            "profile": _profile_selector(self._profile) if self._profile is not None else None,
            "kits": [_kit_selector(item) for item in self._kits],
            "integration_depth": "wrap",
            "coverage": self._coverage(),
            "degradations": list(self._degradations),
        }
        if error is None:
            payload["outcome"] = "succeeded"
            payload["output_digest"] = self._output_digest(output)
        else:
            payload["outcome"] = "aborted"
            payload["error_category"] = f"{type(error).__module__}.{type(error).__name__}"
        return payload

    def complete(
        self,
        output: object = None,
        *,
        outcome: Optional[str] = None,
        host_output: object = None,
        tool_results: object = None,
        usage: object = None,
    ):
        if self._local:
            if self._local_receipt is not None:
                return self._local_receipt
            if self._local_state != "executing":
                raise LeanCtxError("Preview session cannot be completed in its current state")
            if self._local_plan is None:
                raise LeanCtxError("Preview session has no explicit plan")
            resolved_outcome = "unknown" if outcome is None else outcome
            if resolved_outcome not in {"unknown", "accepted", "rejected", "completed", "failed", "aborted"}:
                raise ValueError("outcome must be unknown, accepted, rejected, completed, failed, or aborted")
            self._local_receipt = ContextReceipt.from_session(
                session_id=self._session_id,
                task_id=self._task_id,
                run_id=self._run_id,
                trace_id=self._trace_id,
                plan=self._local_plan,
                view=self._local_view,
                outcome=resolved_outcome,
                host_result=output,
                host_output=host_output,
                tool_results=tool_results,
                usage=usage,
                degradations=tuple(getattr(self, "_local_degradations", ())),
            )
            self._local_state = "completed"
            self._phase = "receipt_ready"
            return self._local_receipt
        if self._receipt is not None:
            return self._receipt
        if self._phase != "executing" or self._session_id is None:
            raise LeanCtxError("session cannot be completed before execution")
        quoted = urllib.parse.quote(self._session_id, safe="")
        data, _ = self._ctx._proxy._post_response(
            f"/v1/sessions/{quoted}/complete", self._terminal_payload(output=output)
        )
        receipt = parse_execution_receipt(
            data, verify_url=self._ctx._proxy.base_url, proxy=self._ctx._proxy
        )
        if receipt.session_id is not None and receipt.session_id != self._session_id:
            raise LeanCtxError("Runtime completion receipt session ID mismatch")
        self._receipt = receipt
        self._phase = "receipt_ready"
        return receipt

    def abort(self, error: BaseException) -> Optional[ExecutionReceipt]:
        if self._local:
            return self._abort_local(error)
        if self._receipt is not None:
            return self._receipt
        if self._phase != "executing" or self._session_id is None:
            return None
        quoted = urllib.parse.quote(self._session_id, safe="")
        data, _ = self._ctx._proxy._post_response(
            f"/v1/sessions/{quoted}/abort", self._terminal_payload(error=error)
        )
        receipt = parse_execution_receipt(
            data, verify_url=self._ctx._proxy.base_url, proxy=self._ctx._proxy
        )
        if receipt.session_id is not None and receipt.session_id != self._session_id:
            raise LeanCtxError("Runtime abort receipt session ID mismatch")
        self._receipt = receipt
        self._phase = "aborted"
        return receipt

    def incomplete_receipt(self, *, outcome: str = "succeeded") -> ExecutionReceipt:
        return make_unsealed_receipt(
            session_id=self._session_id,
            task_id=self._task_id,
            run_id=self._run_id,
            trace_id=self._trace_id,
            agent_id=self._agent_id,
            profile=self._profile,
            kits=self._kits,
            coverage=self._coverage(),
            outcome=outcome,
            degradations=tuple(self._degradations),
        )

    def close(self) -> None:
        """Release client-local binding state; Runtime evidence is untouched."""
        if self._local:
            self._local_state = "closed"
            self._phase = "closed"
            return
        if self._phase != "closed":
            self._headers = None
            self._phase = "closed"

    def __enter__(self) -> "ContextSession":
        if not self._local:
            raise LeanCtxError("context manager support is only available for Preview sessions")
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> bool:
        if exc_value is not None and self._local_receipt is None:
            self._abort_local(exc_value)
        self.close()
        return False
