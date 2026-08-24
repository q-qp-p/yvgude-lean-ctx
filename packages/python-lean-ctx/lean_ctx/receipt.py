"""Immutable Runtime execution receipts and conservative verification."""

from __future__ import annotations

import base64
import hashlib
import json
import math
import re
import urllib.parse
from dataclasses import dataclass, field
from typing import Any, Dict, Mapping, Optional, Tuple

from .errors import LeanCtxError
from .kit import ContextKit, parse_kit
from .profile import TuningProfile, parse_profile

_SHA256 = re.compile(r"^sha256:[0-9a-f]{64}$")
_OPAQUE = re.compile(r"^[\x21-\x7e]{1,256}$")
_COVERAGE = {
    "observed",
    "compressed",
    "context_controlled",
    "full_inline",
    "not_addressable",
}
_EPHEMERAL = {
    "signature",
    "canonical_hash",
    "canonical_json",
    "verify_url",
    "transport",
    "response_headers",
}


def _canonical(value: object) -> bytes:
    try:
        return json.dumps(
            value,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
            allow_nan=False,
        ).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise LeanCtxError(f"invalid canonical receipt JSON: {exc}") from exc


def _opaque(value: object, field_name: str, *, optional: bool = False) -> Optional[str]:
    if value is None and optional:
        return None
    if not isinstance(value, str) or not _OPAQUE.fullmatch(value):
        raise LeanCtxError(f"invalid receipt {field_name}")
    return value


def _optional_string(value: object, field_name: str) -> Optional[str]:
    if value is None:
        return None
    if not isinstance(value, str) or not value:
        raise LeanCtxError(f"invalid receipt {field_name}")
    return value


def _token(value: object, field_name: str) -> Optional[int]:
    if value is None or value == "unknown":
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise LeanCtxError(f"invalid receipt {field_name}")
    return value


@dataclass(frozen=True)
class SavingsInfo:
    original_tokens: Optional[int]
    delivered_tokens: Optional[int]
    saved_tokens: Optional[int]
    saved_pct: Optional[float]
    provider_input_tokens: Optional[int]
    provider_cached_tokens: Optional[int]
    provider_output_tokens: Optional[int]
    reasoning_tokens: Optional[int]
    methodology: str
    baseline_ref: Optional[str]
    quality_status: Optional[str]
    baseline_cost_micros: Optional[int]
    treatment_cost_micros: Optional[int]
    avoided_cost_micros: Optional[int]


@dataclass(frozen=True)
class ExecutionReceipt:
    schema_version: str
    receipt_id: Optional[str]
    session_id: Optional[str]
    task_id: Optional[str]
    run_id: Optional[str]
    trace_id: Optional[str]
    agent_id: Optional[str]
    project_id: Optional[str]
    profile: Optional[TuningProfile]
    kits: Tuple[ContextKit, ...]
    integration_depth: str
    coverage: str
    execution_receipt_ids: Tuple[str, ...]
    canonical_hash: Optional[str]
    signature: Optional[str]
    signer_key_id: Optional[str]
    integrity_status: str
    outcome: str
    degradations: Tuple[str, ...]
    _savings: SavingsInfo
    _canonical_json: bytes
    _verify_url: Optional[str] = field(repr=False, compare=False)

    @property
    def savings(self) -> SavingsInfo:
        return self._savings

    def verify(self) -> bool:
        """Return only affirmative verification; all uncertainty is ``False``."""
        if self.integrity_status != "sealed" or not self._canonical_json:
            return False
        if not isinstance(self.canonical_hash, str) or not _SHA256.fullmatch(self.canonical_hash):
            return False
        digest = "sha256:" + hashlib.sha256(self._canonical_json).hexdigest()
        if digest != self.canonical_hash:
            return False

        public_key = getattr(self, "_public_key", None)
        if self.signature and public_key:
            return _verify_ed25519(self._canonical_json, self.signature, public_key)

        client = getattr(self, "_verify_client", None)
        if client is None and self._verify_url:
            try:
                from .proxy import ProxyClient

                client = ProxyClient(base_url=self._verify_url)
            except Exception:  # pragma: no cover - constructor has no expected failures
                return False
        if client is None:
            # A complete canonical digest is independently useful in local/mock
            # environments that do not expose a verifier endpoint.
            return True
        if not self.receipt_id:
            return False
        try:
            path = "/v1/receipts/{}/verify".format(urllib.parse.quote(self.receipt_id, safe=""))
            response, _ = client._get_response(path)
            return (
                response.get("receipt_id") == self.receipt_id
                and response.get("canonical_hash") == self.canonical_hash
                and response.get("verified") is True
            )
        except Exception:
            return False


@dataclass(frozen=True)
class ContextReceipt:
    """Preview join of factual Engine evidence and an explicit host outcome.

    ``host_result`` is intentionally retained as the exact object returned by
    the host (for example an OpenAI Agents ``RunResult``).  Its presence never
    changes ``outcome``: delivery defaults to ``unknown`` until the host calls
    :meth:`ContextSession.complete` with an explicit outcome.
    """

    preview_version: str
    schema_version: int
    transport_version: Optional[str]
    engine_interface_version: Optional[str]
    session_id: Optional[str]
    task_id: Optional[str]
    run_id: Optional[str]
    trace_id: Optional[str]
    plan: object
    view: object
    outcome: str
    integrity_status: str
    degradations: Tuple[str, ...]
    host_result: object = field(repr=False, compare=False, default=None)
    host_output: object = field(repr=False, compare=False, default=None)
    tool_results: object = field(repr=False, compare=False, default=None)
    host_exception: Optional[BaseException] = field(repr=False, compare=False, default=None)
    usage: object = field(repr=False, compare=False, default=None)

    @classmethod
    def from_session(
        cls,
        *,
        session_id: Optional[str],
        task_id: Optional[str],
        run_id: Optional[str] = None,
        trace_id: Optional[str] = None,
        plan: object,
        view: object,
        outcome: str,
        host_result: object = None,
        host_output: object = None,
        tool_results: object = None,
        host_exception: Optional[BaseException] = None,
        usage: object = None,
        degradations: Tuple[str, ...] = (),
    ) -> "ContextReceipt":
        if outcome not in {"unknown", "accepted", "rejected", "completed", "failed", "aborted"}:
            raise ValueError("invalid Preview host outcome")
        if usage is None:
            usage = _factual_usage(host_result)
        sealed = bool(
            view is not None
            and getattr(view, "receipt_link", None) is not None
            and getattr(view, "integrity_status", "unsealed") == "sealed"
            and not degradations
        )
        status = "sealed" if sealed else "unsealed"
        return cls(
            preview_version="1.0.0",
            schema_version=1,
            transport_version=None if view is None else view.transport_version,
            engine_interface_version=None if view is None else view.engine_interface_version,
            session_id=session_id,
            task_id=task_id,
            run_id=run_id,
            trace_id=trace_id,
            plan=plan,
            view=view,
            outcome=outcome,
            integrity_status=status,
            degradations=tuple(degradations),
            host_result=host_result,
            host_output=host_output,
            tool_results=tool_results,
            host_exception=host_exception,
            usage=usage,
        )

    @property
    def sealed(self) -> bool:
        return self.integrity_status == "sealed"

    @property
    def host_outcome(self) -> str:
        return self.outcome

    @property
    def result(self):
        return self.host_result

    @property
    def run_result(self):
        return self.host_result

    @property
    def output(self):
        return self.host_result if self.host_output is None else self.host_output

    @property
    def source(self):
        return None if self.view is None else self.view.source

    @property
    def invocation(self):
        return None if self.view is None else self.view.invocation

    @property
    def observation(self):
        return None if self.view is None else self.view.observation

    @property
    def engine_invocation(self):
        return self.invocation

    @property
    def engine_observation(self):
        return self.observation

    @property
    def receipt_link(self):
        return None if self.view is None else self.view.receipt_link

    @property
    def recovery_ref(self) -> Optional[str]:
        return None if self.view is None else self.view.recovery_ref

    @property
    def output_digest(self) -> Optional[str]:
        return None if self.view is None else self.view.output_digest

    @property
    def engine_version(self) -> Optional[str]:
        return None if self.view is None else self.view.engine_version

    @property
    def measurements(self):
        return () if self.view is None else self.view.measurements

    @property
    def failure(self):
        return None if self.view is None else self.view.failure

    @property
    def status(self) -> Optional[str]:
        return None if self.view is None else self.view.status

    @property
    def exception(self) -> Optional[BaseException]:
        return self.host_exception

    def verify(self) -> bool:
        """Verify Engine evidence integrity without interpreting host outcome."""
        if not self.sealed or self.view is None:
            return False
        link = self.view.receipt_link
        if link is None or link.invocation_id != self.view.invocation_id:
            return False
        if self.view.status == "succeeded":
            if self.view.text is None or self.view.recovery_ref is None:
                return False
        return True

    def to_dict(self) -> Dict[str, object]:
        """Return a payload-free, deterministic host projection."""
        view = self.view
        result: Dict[str, object] = {
            "preview_version": self.preview_version,
            "schema_version": self.schema_version,
            "transport_version": self.transport_version,
            "engine_interface_version": self.engine_interface_version,
            "session_id": self.session_id,
            "task_id": self.task_id,
            "run_id": self.run_id,
            "trace_id": self.trace_id,
            "outcome": self.outcome,
            "integrity_status": self.integrity_status,
            "degradations": list(self.degradations),
            "plan": None if self.plan is None else self.plan.to_dict(),
            "engine": None,
            "usage": self.usage,
        }
        if view is not None:
            result["engine"] = {
                "source_ref": view.source_ref,
                "source_digest": view.source_digest,
                "recovery_ref": view.recovery_ref,
                "output_ref": view.output_ref,
                "output_digest": view.output_digest,
                "status": view.status,
                "invocation": dict(view.invocation),
                "observation": dict(view.observation),
            }
        if self.host_exception is not None:
            result["host_exception"] = {
                "type": f"{type(self.host_exception).__module__}.{type(self.host_exception).__name__}"
            }
        return result


def _factual_usage(value: object) -> object:
    """Project provider-reported usage only; never derive it from output text."""
    if value is None:
        return None
    usage = getattr(value, "usage", None)
    if usage is None and isinstance(value, Mapping):
        usage = value.get("usage")
    if usage is None:
        return None
    if isinstance(usage, Mapping):
        return dict(usage)
    fields = ("input_tokens", "output_tokens", "total_tokens", "cached_tokens", "reasoning_tokens")
    projected = {}
    for name in fields:
        item = getattr(usage, name, None)
        if item is not None:
            projected[name] = item
    return projected or None


def _decode_bytes(value: object) -> Optional[bytes]:
    if not isinstance(value, str) or not value:
        return None
    try:
        return base64.b64decode(value.removeprefix("base64:"), validate=True)
    except (ValueError, AttributeError):
        try:
            return bytes.fromhex(value.removeprefix("hex:"))
        except ValueError:
            return None


def _verify_ed25519(canonical_json: bytes, signature: str, public_key: str) -> bool:
    try:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError:
        return False
    signature_bytes = _decode_bytes(signature)
    public_key_bytes = _decode_bytes(public_key)
    if signature_bytes is None or public_key_bytes is None:
        return False
    try:
        Ed25519PublicKey.from_public_bytes(public_key_bytes).verify(signature_bytes, canonical_json)
    except Exception:
        return False
    return True


def _receipt_source(payload: Mapping[str, object]) -> Dict[str, object]:
    source: Dict[str, object] = dict(payload)
    for key in ("receipt", "session_receipt"):
        nested = payload.get(key)
        if isinstance(nested, Mapping):
            source.update(nested)
            break
    return source


def _canonical_payload(source: Mapping[str, object]) -> Tuple[bytes, Mapping[str, object]]:
    raw = source.get("canonical_json")
    if raw is None:
        value = {key: value for key, value in source.items() if key not in _EPHEMERAL}
        return _canonical(value), value
    if not isinstance(raw, str):
        raise LeanCtxError("receipt canonical_json must be a JSON string")
    try:
        decoded = json.loads(raw)
    except (ValueError, TypeError) as exc:
        raise LeanCtxError(f"receipt canonical_json is invalid: {exc}") from exc
    if not isinstance(decoded, Mapping):
        raise LeanCtxError("receipt canonical_json must encode an object")
    unsigned = {key: value for key, value in decoded.items() if key not in _EPHEMERAL}
    return _canonical(unsigned), decoded


def _runtime_token(
    savings: Mapping[str, object],
    source: Mapping[str, object],
    field_name: str,
    *fallback_names: str,
) -> Optional[int]:
    """Return a Runtime-issued token or cost value without deriving it locally."""
    for mapping in (savings, source):
        for name in (field_name, *fallback_names):
            if name in mapping:
                return _token(mapping[name], field_name)
    return None


def _savings(value: object, *, source: Mapping[str, object]) -> SavingsInfo:
    if not isinstance(value, Mapping):
        raise LeanCtxError("receipt savings must be an object")
    original = _token(value.get("original_tokens"), "original_tokens")
    delivered = _token(value.get("delivered_tokens"), "delivered_tokens")
    saved = _token(value.get("saved_tokens"), "saved_tokens")
    methodology = value.get("methodology")
    if not isinstance(methodology, str) or not methodology:
        raise LeanCtxError("receipt savings methodology is required")
    if saved is None and methodology == "compression_observation" and original is not None and delivered is not None:
        if original >= delivered:
            saved = original - delivered
    saved_pct_value = value.get("saved_pct")
    if saved_pct_value is None or saved_pct_value == "unknown":
        saved_pct = None
    elif isinstance(saved_pct_value, bool) or not isinstance(saved_pct_value, (int, float)):
        raise LeanCtxError("invalid receipt saved_pct")
    else:
        saved_pct = float(saved_pct_value)
        if not math.isfinite(saved_pct):
            raise LeanCtxError("invalid receipt saved_pct")
    # Cost evidence belongs to the Runtime receipt.  In particular, do not
    # infer a baseline or treatment cost from token headers in this client.
    baseline_cost = _runtime_token(value, source, "baseline_cost_micros")
    treatment_cost = _runtime_token(
        value, source, "treatment_cost_micros", "actual_cost_micros"
    )
    avoided_cost = _runtime_token(value, source, "avoided_cost_micros")
    return SavingsInfo(
        original_tokens=original,
        delivered_tokens=delivered,
        saved_tokens=saved,
        saved_pct=saved_pct,
        provider_input_tokens=_token(value.get("provider_input_tokens"), "provider_input_tokens"),
        provider_cached_tokens=_token(value.get("provider_cached_tokens"), "provider_cached_tokens"),
        provider_output_tokens=_token(value.get("provider_output_tokens"), "provider_output_tokens"),
        reasoning_tokens=_token(value.get("reasoning_tokens"), "reasoning_tokens"),
        methodology=methodology,
        baseline_ref=_optional_string(value.get("baseline_ref"), "baseline_ref"),
        quality_status=_optional_string(value.get("quality_status"), "quality_status"),
        baseline_cost_micros=baseline_cost,
        treatment_cost_micros=treatment_cost,
        avoided_cost_micros=avoided_cost,
    )


def parse_execution_receipt(
    payload: object,
    *,
    verify_url: Optional[str] = None,
    proxy: object = None,
) -> ExecutionReceipt:
    """Parse the sealed Runtime response into the frozen Python evidence view."""
    if not isinstance(payload, Mapping):
        raise LeanCtxError("malformed Runtime receipt response")
    source = _receipt_source(payload)
    canonical_json, canonical_object = _canonical_payload(source)
    for key, value in canonical_object.items():
        source.setdefault(key, value)

    schema_version = source.get("schema_version")
    integration_depth = source.get("integration_depth")
    coverage = source.get("coverage")
    integrity_status = source.get("integrity_status")
    outcome = source.get("outcome")
    if not all(isinstance(value, str) and value for value in (schema_version, integration_depth, coverage, integrity_status, outcome)):
        raise LeanCtxError("receipt required field missing")
    if coverage not in _COVERAGE:
        raise LeanCtxError("invalid receipt coverage")

    raw_kits = source.get("kits", ())
    if not isinstance(raw_kits, (list, tuple)):
        raise LeanCtxError("receipt kits must be an array")
    try:
        kits = tuple(parse_kit(kit) for kit in raw_kits)
    except LeanCtxError:
        raise
    profile_value = source.get("profile")
    try:
        profile = None if profile_value is None else parse_profile(profile_value)
    except ValueError as exc:
        raise LeanCtxError(str(exc)) from exc

    execution_ids = source.get("execution_receipt_ids", ())
    if not isinstance(execution_ids, (list, tuple)):
        raise LeanCtxError("receipt execution_receipt_ids must be an array")
    parsed_execution_ids = tuple(_opaque(value, "execution_receipt_id") for value in execution_ids)
    degradations = source.get("degradations", ())
    if not isinstance(degradations, (list, tuple)) or not all(
        isinstance(value, str) and value for value in degradations
    ):
        raise LeanCtxError("receipt degradations must be an array of strings")
    canonical_hash = _optional_string(source.get("canonical_hash"), "canonical_hash")
    if canonical_hash is not None and not _SHA256.fullmatch(canonical_hash):
        raise LeanCtxError("invalid receipt canonical_hash")
    savings_value = source.get("savings", source.get("savings_info"))
    receipt = ExecutionReceipt(
        schema_version=schema_version,
        receipt_id=_opaque(source.get("receipt_id"), "receipt_id", optional=True),
        session_id=_opaque(source.get("session_id"), "session_id", optional=True),
        task_id=_opaque(source.get("task_id"), "task_id", optional=True),
        run_id=_opaque(source.get("run_id"), "run_id", optional=True),
        trace_id=_opaque(source.get("trace_id"), "trace_id", optional=True),
        agent_id=_opaque(source.get("agent_id"), "agent_id", optional=True),
        project_id=_opaque(source.get("project_id"), "project_id", optional=True),
        profile=profile,
        kits=kits,
        integration_depth=integration_depth,
        coverage=coverage,
        execution_receipt_ids=parsed_execution_ids,
        canonical_hash=canonical_hash,
        signature=_optional_string(source.get("signature"), "signature"),
        signer_key_id=_optional_string(source.get("signer_key_id"), "signer_key_id"),
        integrity_status=integrity_status,
        outcome=outcome,
        degradations=tuple(degradations),
        _savings=_savings(savings_value, source=source),
        _canonical_json=canonical_json,
        _verify_url=verify_url.rstrip("/") if verify_url else None,
    )
    if proxy is not None:
        object.__setattr__(receipt, "_verify_client", proxy)
    public_key = source.get("signer_public_key", source.get("public_key"))
    if isinstance(public_key, str):
        object.__setattr__(receipt, "_public_key", public_key)
    return receipt


def make_unsealed_receipt(
    *,
    session_id: Optional[str],
    task_id: Optional[str],
    run_id: Optional[str],
    trace_id: Optional[str],
    agent_id: Optional[str],
    profile: Optional[TuningProfile],
    kits: Tuple[ContextKit, ...],
    coverage: str,
    outcome: str,
    degradations: Tuple[str, ...],
) -> ExecutionReceipt:
    """Create the only permitted local fallback: explicitly unsealed evidence."""
    return ExecutionReceipt(
        schema_version="1",
        receipt_id=None,
        session_id=session_id,
        task_id=task_id,
        run_id=run_id,
        trace_id=trace_id,
        agent_id=agent_id,
        project_id=None,
        profile=profile,
        kits=kits,
        integration_depth="wrap",
        coverage=coverage,
        execution_receipt_ids=(),
        canonical_hash=None,
        signature=None,
        signer_key_id=None,
        integrity_status="unsealed",
        outcome=outcome,
        degradations=degradations,
        _savings=SavingsInfo(
            original_tokens=None,
            delivered_tokens=None,
            saved_tokens=None,
            saved_pct=None,
            provider_input_tokens=None,
            provider_cached_tokens=None,
            provider_output_tokens=None,
            reasoning_tokens=None,
            methodology="unavailable",
            baseline_ref=None,
            quality_status=None,
            baseline_cost_micros=None,
            treatment_cost_micros=None,
            avoided_cost_micros=None,
        ),
        _canonical_json=b"",
        _verify_url=None,
    )
