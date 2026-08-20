"""Task-scoped synchronous session handle for the v1 Runtime protocol."""

from __future__ import annotations

import hashlib
import json
import uuid
import urllib.parse
from types import MappingProxyType
from typing import TYPE_CHECKING, Any, Dict, Mapping, Optional, Tuple

from .errors import LeanCtxError
from .kit import ContextKit, parse_kit
from .profile import TuningProfile, parse_profile
from .proxy import ProxyHTTPResponse, ProxyObservation, parse_proxy_observation
from .receipt import ExecutionReceipt, make_unsealed_receipt, parse_execution_receipt

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

    def __init__(self, ctx: "LeanCTX") -> None:
        self._ctx = ctx
        self._phase = "pending"
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

    @staticmethod
    def validate_task(task: object) -> str:
        return _task(task)

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

    def complete(self, output: object) -> ExecutionReceipt:
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
        if self._phase != "closed":
            self._headers = None
            self._phase = "closed"
