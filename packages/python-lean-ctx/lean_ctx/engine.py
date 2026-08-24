"""Strict local Engine Interface v1 transport for the Python Preview.

The Engine owns source access and factual observations.  This module owns only
the process boundary and immutable Python projections; task/session lifecycle,
planning, host execution, and host outcomes remain in :mod:`lean_ctx.session`.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import tempfile
from collections.abc import Mapping
from dataclasses import dataclass, field
from pathlib import Path
from types import MappingProxyType
from typing import Optional, Sequence, Tuple, TYPE_CHECKING

from .errors import (
    EngineExecutionError,
    EngineProtocolError,
    EngineRejectedError,
    EngineTimeoutError,
    EngineUnavailableError,
    LeanCtxEngineExecutionError,
    LeanCtxEngineProtocolError,
    LeanCtxEngineRejected,
    LeanCtxEngineTimeout,
    LeanCtxEngineUnavailable,
)
from .receipt import ContextReceipt

if TYPE_CHECKING:  # pragma: no cover
    from .session import ContextSession


PREVIEW_VERSION = "1.0.0"
SCHEMA_VERSION = 1
TRANSPORT_VERSION = "1.0.0"
WIRE_TRANSPORT_VERSION = 1
ENGINE_INTERFACE_VERSION = "1.0.0"
ENGINE_ID = "lean-ctx-local"
ENGINE_VERSION_MAJOR = 3
CAPABILITY_ID = "capability://leanctx/context-optimization"
CAPABILITY_VERSION = "1.0.0"
ENGINE_BINARY_ENV = "LEAN_CTX_ENGINE_BINARY"

_MAX_PATH_BYTES = 4096
_MAX_REF_BYTES = 512
_MAX_VIEW_BYTES = 8 * 1024 * 1024
_MAX_REQUEST_BYTES = 64 * 1024
_SHA256 = re.compile(r"^sha256:[0-9a-f]{64}$")
_SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
_OPAQUE = re.compile(r"^[\x21-\x7e]+$")
_FAILURE_CODES = {
    "policy_rejected",
    "source_unavailable",
    "source_integrity_mismatch",
    "resource_limit",
    "unsupported_operation",
    "internal",
}
_STATUSES = {"succeeded", "degraded", "rejected", "failed"}
_CLASSIFICATIONS = {"measured", "estimated", "unavailable"}


def _error(message: str) -> LeanCtxEngineProtocolError:
    return LeanCtxEngineProtocolError(message)


def _strict_mapping(
    value: object,
    field_name: str,
    *,
    required: Sequence[str],
    optional: Sequence[str] = (),
) -> Mapping[str, object]:
    if not isinstance(value, Mapping):
        raise _error(f"{field_name} must be an object")
    allowed = set(required) | set(optional)
    unknown = set(value) - allowed
    if unknown:
        raise _error(f"{field_name} contains unknown field {sorted(unknown)[0]}")
    missing = [name for name in required if name not in value]
    if missing:
        raise _error(f"{field_name} missing required field {missing[0]}")
    if any(not isinstance(key, str) for key in value):
        raise _error(f"{field_name} keys must be strings")
    return value


def _string(value: object, field_name: str, *, max_bytes: int = _MAX_REF_BYTES) -> str:
    if not isinstance(value, str) or not value or "\x00" in value:
        raise _error(f"{field_name} must be a non-empty string")
    if len(value.encode("utf-8")) > max_bytes:
        raise _error(f"{field_name} exceeds its size bound")
    return value


def _opaque(value: object, field_name: str) -> str:
    value = _string(value, field_name)
    if not _OPAQUE.fullmatch(value):
        raise _error(f"{field_name} must be printable ASCII")
    return value


def _digest(value: object, field_name: str) -> str:
    value = _string(value, field_name, max_bytes=71)
    if not _SHA256.fullmatch(value):
        raise _error(f"{field_name} must be a lowercase SHA-256 digest")
    return value


def _schema_version(value: object, field_name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value != SCHEMA_VERSION:
        raise _error(f"{field_name} must be schema version {SCHEMA_VERSION}")
    return value


def _version(value: object, field_name: str, *, major: Optional[int] = None) -> str:
    value = _string(value, field_name, max_bytes=64)
    if not _SEMVER.fullmatch(value):
        raise _error(f"{field_name} must be a semantic version")
    try:
        actual_major = int(value.split(".", 1)[0])
    except ValueError:  # pragma: no cover - guarded by the regexp
        raise _error(f"{field_name} must be a semantic version")
    if major is not None and major != actual_major:
        raise _error(f"{field_name} major version is unsupported")
    return value


def _transport_version(value: object, field_name: str) -> str:
    if isinstance(value, bool):
        raise _error(f"{field_name} is unsupported")
    if isinstance(value, int):
        if value != WIRE_TRANSPORT_VERSION:
            raise _error(f"{field_name} is unsupported")
        return TRANSPORT_VERSION
    return _version(value, field_name, major=1)


def _ref(value: object, field_name: str) -> str:
    return _opaque(value, field_name)


def _sha256_text(value: str) -> str:
    return "sha256:" + hashlib.sha256(value.encode("utf-8")).hexdigest()


def _bounded_text(value: object, field_name: str, *, allow_none: bool = False) -> Optional[str]:
    if value is None and allow_none:
        return None
    if not isinstance(value, str):
        raise _error(f"{field_name} must be text")
    if len(value.encode("utf-8")) > _MAX_VIEW_BYTES:
        raise _error(f"{field_name} exceeds the bounded view size")
    if "\x00" in value:
        raise _error(f"{field_name} contains NUL")
    return value


def _list_of_strings(value: object, field_name: str, *, max_items: int = 32) -> Tuple[str, ...]:
    if not isinstance(value, (list, tuple)) or len(value) == 0 or len(value) > max_items:
        raise _error(f"{field_name} must contain 1..{max_items} strings")
    result = tuple(_ref(item, f"{field_name}[{index}]") for index, item in enumerate(value))
    if len(set(result)) != len(result):
        raise _error(f"{field_name} contains duplicate references")
    return result


@dataclass(frozen=True)
class ContextSource:
    """One explicit, rooted local source selected by the host."""

    path: str
    project_root: Optional[str] = None
    media_type: str = "text/plain"
    source_ref: Optional[str] = None
    source_digest: Optional[str] = None

    def __post_init__(self) -> None:
        raw_path = os.fspath(self.path)
        if not isinstance(raw_path, str) or not raw_path or "\x00" in raw_path:
            raise ValueError("source path must be a non-empty path without NUL")
        if len(raw_path.encode("utf-8")) > _MAX_PATH_BYTES:
            raise ValueError("source path exceeds the bounded path size")
        raw_root = os.fspath(self.project_root) if self.project_root is not None else os.getcwd()
        if not isinstance(raw_root, str) or not raw_root or "\x00" in raw_root:
            raise ValueError("project_root must be a non-empty path without NUL")
        root = str(Path(raw_root).expanduser().resolve(strict=False))
        # Preserve source symlink components for the rooted Engine to reject;
        # resolving them here would erase the security-relevant request shape.
        path = os.path.abspath(os.path.expanduser(raw_path))
        if len(path.encode("utf-8")) > _MAX_PATH_BYTES:
            raise ValueError("resolved source path exceeds the bounded path size")
        if not isinstance(self.media_type, str) or not self.media_type.strip():
            raise ValueError("media_type must be a non-empty string")
        if self.source_ref is not None:
            _ref(self.source_ref, "source_ref")
        if self.source_digest is not None:
            _digest(self.source_digest, "source_digest")
        object.__setattr__(self, "path", path)
        object.__setattr__(self, "project_root", root)

    @property
    def relative_path(self) -> str:
        """Return the request path only when it remains inside project_root."""
        root = Path(self.project_root or os.getcwd())
        path = Path(self.path)
        try:
            relative = path.relative_to(root)
        except ValueError as exc:
            raise _error("source is outside the declared project root") from exc
        value = relative.as_posix()
        if not value or value == "." or value.startswith("../"):
            raise _error("source path is not a rooted relative path")
        return value

    def descriptor(self) -> Mapping[str, object]:
        result = {
            "path": self.relative_path,
            "media_type": self.media_type,
        }
        if self.source_ref is not None:
            result["source_ref"] = self.source_ref
        if self.source_digest is not None:
            result["source_digest"] = self.source_digest
        return result

    def to_dict(self) -> Mapping[str, object]:
        return self.descriptor()


@dataclass(frozen=True)
class ContextMeasurement:
    """One factual Engine measurement; no local arithmetic is performed."""

    name: str
    unit: str
    classification: str
    value: Optional[int]


@dataclass(frozen=True)
class ContextFailure:
    """The Engine's stable failure taxonomy."""

    code: str
    retryable_by_host: bool
    recovery_ref: Optional[str]


@dataclass(frozen=True)
class ContextReceiptLink:
    """Integrity-addressed link from an observation to the Engine receipt."""

    schema_version: int
    receipt_id: str
    receipt_ref: str
    receipt_digest: str
    invocation_id: str


class RecoveredSource(str):
    """Exact source bytes returned by Engine recovery, with verified metadata."""

    def __new__(
        cls,
        text: str,
        *,
        source_ref: str,
        source_digest: str,
        recovery_ref: str,
    ):
        value = str.__new__(cls, text)
        value.source_ref = source_ref
        value.source_digest = source_digest
        value.recovery_ref = recovery_ref
        return value

    @property
    def text(self) -> str:
        return str(self)


@dataclass(frozen=True)
class ContextView:
    """Bounded Engine output joined to its admitted source and receipt."""

    source: ContextSource
    text: Optional[str]
    output_ref: Optional[str]
    output_digest: Optional[str]
    source_ref: str
    source_digest: str
    recovery_ref: Optional[str]
    status: str
    measurements: Tuple[ContextMeasurement, ...]
    failure: Optional[ContextFailure]
    receipt_link: Optional[ContextReceiptLink]
    invocation: Mapping[str, object]
    observation: Mapping[str, object]
    schema_version: int
    transport_version: str
    engine_interface_version: str
    _engine: "LocalEngineClient" = field(repr=False, compare=False)

    @property
    def integrity_status(self) -> str:
        sealed = self.receipt_link is not None and (
            self.status != "succeeded" or self.recovery_ref is not None
        )
        return "sealed" if sealed else "unsealed"

    @property
    def input_ref(self) -> str:
        return str(self.invocation["input_ref"])

    @property
    def input_digest(self) -> str:
        return str(self.invocation["input_digest"])

    @property
    def invocation_id(self) -> str:
        return str(self.invocation["invocation_id"])

    @property
    def engine_version(self) -> str:
        return str(self.invocation["engine"]["engine_version"])

    @property
    def capability_version(self) -> str:
        return str(self.invocation["operation"]["capability_version"])

    def require_text(self) -> str:
        if self.text is None:
            code = self.failure.code if self.failure is not None else "unknown"
            raise LeanCtxEngineExecutionError(f"Engine view has no text ({code})", view=self)
        return self.text

    @property
    def recovery(self) -> Mapping[str, object]:
        return {
            "recovery_ref": self.recovery_ref,
            "source_ref": self.source_ref,
            "source_digest": self.source_digest,
        }

    def to_dict(self) -> Mapping[str, object]:
        """Return the validated response projection without adding fields."""
        return {
            "schema_version": self.schema_version,
            "transport_version": self.transport_version,
            "engine_interface_version": self.engine_interface_version,
            "view": {
                "text": self.text,
                "output_ref": self.output_ref,
                "output_digest": self.output_digest,
            },
            "invocation": dict(self.invocation),
            "observation": dict(self.observation),
            "recovery": dict(self.recovery),
        }

    def recover(self) -> RecoveredSource:
        """Recover and verify the exact source admitted for this view."""
        if self.recovery_ref is None:
            raise LeanCtxEngineProtocolError("view has no recovery reference")
        recovered = self._engine.recover(
            project_root=self.source.project_root or os.getcwd(),
            path=self.source.relative_path,
            recovery_ref=self.recovery_ref,
            source_ref=self.source_ref,
            source_digest=self.source_digest,
        )
        if recovered.source_ref != self.source_ref:
            raise LeanCtxEngineProtocolError("recovery source reference does not match view")
        if recovered.source_digest != self.source_digest:
            raise LeanCtxEngineProtocolError("recovery source digest does not match view")
        return recovered

    @classmethod
    def from_response(
        cls,
        payload: object,
        *,
        source: ContextSource,
        engine: "LocalEngineClient",
    ) -> "ContextView":
        top = _strict_mapping(
            payload,
            "Engine response",
            required=(
                "schema_version",
                "transport_version",
                "engine_interface_version",
                "view",
                "invocation",
                "observation",
                "recovery",
            ),
        )
        schema_version = _schema_version(top["schema_version"], "schema_version")
        transport_version = _transport_version(top["transport_version"], "transport_version")
        engine_interface_version = _version(
            top["engine_interface_version"], "engine_interface_version"
        )
        if transport_version != TRANSPORT_VERSION:
            raise _error("transport_version is not the pinned Preview version")
        if engine_interface_version != ENGINE_INTERFACE_VERSION:
            raise _error("engine_interface_version is not the pinned Preview version")
        invocation = _parse_invocation(top["invocation"])
        observation = _parse_observation(top["observation"], invocation)
        recovery = _parse_recovery(top["recovery"], invocation, observation)
        view_payload = top["view"]
        if view_payload is None:
            view_data = None
        else:
            view_data = _strict_mapping(
                view_payload,
                "view",
                required=("text", "output_ref", "output_digest"),
            )
        text_value: Optional[str]
        output_ref: Optional[str]
        output_digest: Optional[str]
        if view_data is None:
            text_value = output_ref = output_digest = None
        else:
            text_value = _bounded_text(
                view_data["text"], "view.text", allow_none=observation["status"] != "succeeded"
            )
            output_ref = None if view_data["output_ref"] is None else _ref(
                view_data["output_ref"], "view.output_ref"
            )
            output_digest = None if view_data["output_digest"] is None else _digest(
                view_data["output_digest"], "view.output_digest"
            )
        observation_output_ref = observation.get("output_ref")
        observation_output_digest = observation.get("output_digest")
        if output_ref != observation_output_ref or output_digest != observation_output_digest:
            raise _error("view output does not match Engine observation")
        if observation["status"] == "succeeded":
            if text_value is None or output_digest is None or output_ref is None:
                raise _error("succeeded Engine observation requires a bounded view")
            if _sha256_text(text_value) != output_digest:
                raise _error("view text digest does not match output_digest")
            if output_ref != "output:" + output_digest.removeprefix("sha256:"):
                raise _error("view output_ref does not bind output_digest")
        elif text_value is not None and output_digest is not None and _sha256_text(text_value) != output_digest:
            raise _error("failed view text digest does not match output_digest")

        link = _parse_receipt_link(observation.get("receipt_link"), invocation)
        failure = _parse_failure(observation.get("failure"))
        if observation["status"] == "succeeded" and link is None:
            raise LeanCtxEngineProtocolError("succeeded Engine observation has no receipt link")
        if failure is not None and failure.recovery_ref is not None:
            if recovery["recovery_ref"] != failure.recovery_ref:
                raise _error("failure recovery_ref does not match recovery descriptor")
        source_ref = recovery["source_ref"]
        if source_ref not in invocation["source_refs"]:
            raise _error("recovery source_ref is not admitted by invocation")
        source_digest = recovery["source_digest"]
        bound_source = source
        if source.source_ref is not None and source.source_ref != source_ref:
            raise _error("Engine source_ref does not match requested source")
        if source.source_digest is not None and source.source_digest != source_digest:
            raise _error("Engine source_digest does not match requested source")
        bound_source = ContextSource(
            path=source.path,
            project_root=source.project_root,
            media_type=source.media_type,
            source_ref=source_ref,
            source_digest=source_digest,
        )
        measurements = tuple(_parse_measurement(item, index) for index, item in enumerate(observation["measurements"]))
        failure_obj = None if failure is None else ContextFailure(**failure)
        link_obj = None if link is None else ContextReceiptLink(**link)
        return cls(
            source=bound_source,
            text=text_value,
            output_ref=output_ref,
            output_digest=output_digest,
            source_ref=source_ref,
            source_digest=source_digest,
            recovery_ref=recovery["recovery_ref"],
            status=observation["status"],
            measurements=measurements,
            failure=failure_obj,
            receipt_link=link_obj,
            invocation=MappingProxyType(dict(invocation)),
            observation=MappingProxyType(dict(observation)),
            schema_version=schema_version,
            transport_version=transport_version,
            engine_interface_version=engine_interface_version,
            _engine=engine,
        )


@dataclass(frozen=True)
class ContextPlan:
    """Explicit host intent; Engine never returns or mutates this plan."""

    session: "ContextSession" = field(repr=False, compare=False)
    task_id: str
    source: ContextSource
    plan_id: str
    mode: str = "aggressive"
    budget_tokens: Optional[int] = None
    preview_version: str = PREVIEW_VERSION
    engine_interface_version: str = ENGINE_INTERFACE_VERSION

    def to_dict(self) -> Mapping[str, object]:
        result = {
            "plan_id": self.plan_id,
            "task_id": self.task_id,
            "source": self.source.descriptor(),
            "mode": self.mode,
            "engine_interface_version": self.engine_interface_version,
            "preview_version": self.preview_version,
        }
        if self.budget_tokens is not None:
            result["budget_tokens"] = self.budget_tokens
        return result

    def execute(self) -> ContextView:
        return self.session._execute_local_plan(self)


def _parse_invocation(value: object) -> Mapping[str, object]:
    invocation = _strict_mapping(
        value,
        "invocation",
        required=(
            "schema_version",
            "invocation_id",
            "engine",
            "operation",
            "input_ref",
            "input_digest",
            "source_refs",
            "policy_admission",
        ),
    )
    _schema_version(invocation["schema_version"], "invocation.schema_version")
    invocation_id = _opaque(invocation["invocation_id"], "invocation.invocation_id")
    engine = _strict_mapping(
        invocation["engine"], "invocation.engine", required=("engine_id", "engine_version")
    )
    engine_id = _opaque(engine["engine_id"], "invocation.engine.engine_id")
    engine_version = _version(
        engine["engine_version"],
        "invocation.engine.engine_version",
        major=ENGINE_VERSION_MAJOR,
    )
    if engine_id != ENGINE_ID:
        raise _error("invocation.engine identity is not pinned")
    operation = _strict_mapping(
        invocation["operation"],
        "invocation.operation",
        required=("capability_id", "capability_version"),
    )
    capability_id = _ref(operation["capability_id"], "invocation.operation.capability_id")
    capability_version = _version(
        operation["capability_version"], "invocation.operation.capability_version", major=1
    )
    if capability_id != CAPABILITY_ID or capability_version != CAPABILITY_VERSION:
        raise _error("invocation.operation capability is not pinned")
    input_ref = _ref(invocation["input_ref"], "invocation.input_ref")
    input_digest = _digest(invocation["input_digest"], "invocation.input_digest")
    source_refs = _list_of_strings(invocation["source_refs"], "invocation.source_refs")
    if input_ref not in source_refs:
        raise _error("invocation.source_refs must contain input_ref")
    admission = _strict_mapping(
        invocation["policy_admission"],
        "invocation.policy_admission",
        required=("policy_ref", "decision"),
    )
    _ref(admission["policy_ref"], "invocation.policy_admission.policy_ref")
    if admission["decision"] not in {"admitted", "rejected"}:
        raise _error("invocation.policy_admission.decision is invalid")
    return {
        **dict(invocation),
        "invocation_id": invocation_id,
        "input_ref": input_ref,
        "input_digest": input_digest,
        "source_refs": source_refs,
        "engine": dict(engine),
        "operation": dict(operation),
        "policy_admission": dict(admission),
    }


def _parse_observation(value: object, invocation: Mapping[str, object]) -> Mapping[str, object]:
    observation = _strict_mapping(
        value,
        "observation",
        required=(
            "schema_version",
            "invocation_id",
            "status",
            "source_lineage",
            "measurements",
        ),
        optional=("output_ref", "output_digest", "failure", "receipt_link"),
    )
    _schema_version(observation["schema_version"], "observation.schema_version")
    invocation_id = _opaque(observation["invocation_id"], "observation.invocation_id")
    if invocation_id != invocation["invocation_id"]:
        raise _error("observation invocation_id does not match invocation")
    status = observation["status"]
    if status not in _STATUSES:
        raise _error("observation.status is invalid")
    source_lineage = _list_of_strings(observation["source_lineage"], "observation.source_lineage")
    if any(item not in invocation["source_refs"] for item in source_lineage):
        raise _error("observation.source_lineage is not admitted by invocation")
    measurements = observation["measurements"]
    if not isinstance(measurements, (list, tuple)) or len(measurements) > 32:
        raise _error("observation.measurements exceeds its bound")
    output_ref = observation.get("output_ref")
    output_digest = observation.get("output_digest")
    if (output_ref is None) != (output_digest is None):
        raise _error("observation output_ref/output_digest must appear together")
    if output_ref is not None:
        output_ref = _ref(output_ref, "observation.output_ref")
        output_digest = _digest(output_digest, "observation.output_digest")
    failure = _parse_failure(observation.get("failure"))
    if status == "succeeded" and (output_ref is None or failure is not None):
        raise _error("succeeded observation requires output and no failure")
    if status == "degraded" and (output_ref is None or failure is None):
        raise _error("degraded observation requires output and failure")
    if status in {"failed", "rejected"} and (output_ref is not None or failure is None):
        raise _error("failed/rejected observation requires failure without output")
    if status == "rejected" and failure["code"] != "policy_rejected":
        raise _error("rejected observation must use policy_rejected")
    if invocation["policy_admission"]["decision"] == "rejected" and status != "rejected":
        raise _error("rejected policy admission must produce rejected observation")
    if invocation["policy_admission"]["decision"] == "admitted" and status == "rejected":
        raise _error("admitted invocation cannot produce rejected observation")
    return {
        **dict(observation),
        "invocation_id": invocation_id,
        "status": status,
        "source_lineage": source_lineage,
        "output_ref": output_ref,
        "output_digest": output_digest,
        "failure": failure,
    }


def _parse_failure(value: object) -> Optional[Mapping[str, object]]:
    if value is None:
        return None
    failure = _strict_mapping(
        value,
        "failure",
        required=("code", "retryable_by_host"),
        optional=("recovery_ref",),
    )
    code = failure["code"]
    if code not in _FAILURE_CODES:
        raise _error("failure.code is invalid")
    if not isinstance(failure["retryable_by_host"], bool):
        raise _error("failure.retryable_by_host must be boolean")
    recovery_ref = failure.get("recovery_ref")
    if recovery_ref is not None:
        recovery_ref = _ref(recovery_ref, "failure.recovery_ref")
    if code == "policy_rejected" and (failure["retryable_by_host"] or recovery_ref is not None):
        raise _error("policy_rejected cannot be retryable or recoverable")
    if code in {"source_unavailable", "source_integrity_mismatch"} and recovery_ref is None:
        raise _error("source failures require recovery_ref")
    return {"code": code, "retryable_by_host": failure["retryable_by_host"], "recovery_ref": recovery_ref}


def _parse_measurement(value: object, index: int) -> ContextMeasurement:
    measurement = _strict_mapping(
        value,
        f"observation.measurements[{index}]",
        required=("name", "unit", "classification"),
        optional=("value",),
    )
    name = _string(measurement["name"], "measurement.name", max_bytes=64)
    unit = _string(measurement["unit"], "measurement.unit", max_bytes=64)
    if not re.fullmatch(r"[a-z0-9_-]+", name) or not re.fullmatch(r"[a-z0-9_-]+", unit):
        raise _error("measurement names and units must be lowercase ASCII")
    classification = measurement["classification"]
    if classification not in _CLASSIFICATIONS:
        raise _error("measurement.classification is invalid")
    value_item = measurement.get("value")
    if value_item is not None and (isinstance(value_item, bool) or not isinstance(value_item, int) or value_item < 0):
        raise _error("measurement.value must be a non-negative integer")
    if classification == "unavailable" and value_item is not None:
        raise _error("unavailable measurement cannot contain a value")
    if classification != "unavailable" and value_item is None:
        raise _error("measured/estimated measurement requires a value")
    return ContextMeasurement(name, unit, classification, value_item)


def _parse_receipt_link(value: object, invocation: Mapping[str, object]) -> Optional[Mapping[str, object]]:
    if value is None:
        return None
    link = _strict_mapping(
        value,
        "observation.receipt_link",
        required=("schema_version", "receipt_id", "receipt_ref", "receipt_digest", "invocation_id"),
    )
    _schema_version(link["schema_version"], "receipt_link.schema_version")
    receipt_id = _opaque(link["receipt_id"], "receipt_link.receipt_id")
    receipt_ref = _ref(link["receipt_ref"], "receipt_link.receipt_ref")
    receipt_digest = _digest(link["receipt_digest"], "receipt_link.receipt_digest")
    invocation_id = _opaque(link["invocation_id"], "receipt_link.invocation_id")
    if invocation_id != invocation["invocation_id"]:
        raise _error("receipt link invocation_id does not match invocation")
    expected_ref = "receipt:sha256:" + receipt_digest.removeprefix("sha256:")
    if receipt_ref != expected_ref:
        raise _error("receipt link does not bind receipt_digest")
    return {
        "schema_version": SCHEMA_VERSION,
        "receipt_id": receipt_id,
        "receipt_ref": receipt_ref,
        "receipt_digest": receipt_digest,
        "invocation_id": invocation_id,
    }


def _parse_recovery(
    value: object,
    invocation: Mapping[str, object],
    observation: Mapping[str, object],
) -> Mapping[str, object]:
    recovery = _strict_mapping(
        value,
        "recovery",
        required=("recovery_ref", "source_ref", "source_digest"),
    )
    recovery_ref = recovery["recovery_ref"]
    if recovery_ref is not None:
        recovery_ref = _ref(recovery_ref, "recovery.recovery_ref")
    if observation["status"] == "succeeded" and recovery_ref is None:
        raise _error("succeeded Engine observation requires exact recovery")
    source_ref = _ref(recovery["source_ref"], "recovery.source_ref")
    source_digest = _digest(recovery["source_digest"], "recovery.source_digest")
    if source_ref not in invocation["source_refs"]:
        raise _error("recovery.source_ref is not in invocation.source_refs")
    failure = observation.get("failure")
    if failure is not None and failure.get("recovery_ref") is not None:
        if recovery_ref != failure["recovery_ref"]:
            raise _error("recovery.recovery_ref does not match observation failure")
    return {
        "recovery_ref": recovery_ref,
        "source_ref": source_ref,
        "source_digest": source_digest,
    }


class LocalEngineClient:
    """Subprocess transport for the versioned local Engine CLI."""

    def __init__(
        self,
        binary: Optional[str] = None,
        *,
        timeout: float = 30.0,
    ) -> None:
        selected = binary if binary is not None else "lean-ctx"
        if not isinstance(selected, str) or not selected.strip():
            raise ValueError("engine binary must be a non-empty string")
        if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or timeout <= 0:
            raise ValueError("engine timeout must be greater than zero")
        self.binary = selected
        self.timeout = float(timeout)

    def context_view(self, plan: ContextPlan) -> ContextView:
        source = plan.source
        request = {
            "schema_version": SCHEMA_VERSION,
            "transport_version": WIRE_TRANSPORT_VERSION,
            "engine_interface_version": plan.engine_interface_version,
            "path": source.relative_path,
            "mode": plan.mode,
        }
        if plan.budget_tokens is not None:
            raise LeanCtxEngineProtocolError(
                "Engine transport v1 does not accept a token-budget override"
            )
        payload = self._run("context-view", source.project_root or os.getcwd(), request)
        return ContextView.from_response(payload, source=source, engine=self)

    def recover(
        self,
        *,
        project_root: str,
        path: str,
        recovery_ref: str,
        source_ref: str,
        source_digest: str,
    ) -> RecoveredSource:
        request = {
            "schema_version": SCHEMA_VERSION,
            "transport_version": WIRE_TRANSPORT_VERSION,
            "engine_interface_version": ENGINE_INTERFACE_VERSION,
            "path": _string(path, "path", max_bytes=_MAX_PATH_BYTES),
            "recovery_ref": _ref(recovery_ref, "recovery_ref"),
            "source_ref": _ref(source_ref, "source_ref"),
            "source_digest": _digest(source_digest, "source_digest"),
        }
        payload = self._run("recover", project_root, request)
        return _parse_recovery_response(
            payload,
            expected_recovery_ref=recovery_ref,
            expected_source_ref=source_ref,
            expected_source_digest=source_digest,
        )

    def _run(self, operation: str, project_root: str, request: Mapping[str, object]) -> object:
        if operation not in {"context-view", "recover"}:
            raise ValueError("unsupported Engine operation")
        root = str(Path(project_root).expanduser().resolve(strict=False))
        encoded = json.dumps(request, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        if len(encoded.encode("utf-8")) > _MAX_REQUEST_BYTES:
            raise LeanCtxEngineProtocolError("Engine request exceeds its size bound")
        temporary_path = None
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8", prefix="leanctx-engine-", suffix=".json", delete=False
            ) as handle:
                temporary_path = handle.name
                handle.write(encoded)
                handle.flush()
                os.fchmod(handle.fileno(), 0o600)
            try:
                completed = subprocess.run(
                    [
                        self.binary,
                        "engine",
                        operation,
                        "--project-root",
                        root,
                        "--json-file",
                        temporary_path,
                    ],
                    capture_output=True,
                    text=True,
                    cwd=root,
                    timeout=self.timeout,
                    check=False,
                )
            except FileNotFoundError as exc:
                raise LeanCtxEngineUnavailable("local Engine executable is unavailable") from exc
            except PermissionError as exc:
                raise LeanCtxEngineUnavailable("local Engine executable is unavailable") from exc
            except subprocess.TimeoutExpired as exc:
                raise LeanCtxEngineTimeout("local Engine exceeded its deadline") from exc
            except OSError as exc:
                raise LeanCtxEngineUnavailable("local Engine could not be started") from exc
            if completed.returncode != 0:
                code = _engine_error_code(completed.stderr)
                if code == "policy_rejected":
                    raise LeanCtxEngineRejected("local Engine rejected the request")
                if code in {"source_unavailable", "request_file_unavailable"}:
                    raise LeanCtxEngineUnavailable("local Engine source is unavailable")
                raise LeanCtxEngineProtocolError(
                    "local Engine rejected an unsafe or incompatible request"
                )
            stdout = completed.stdout
            if not isinstance(stdout, str) or not stdout.strip():
                raise LeanCtxEngineProtocolError("local Engine returned no JSON response")
            try:
                return json.loads(stdout)
            except (TypeError, ValueError) as exc:
                raise LeanCtxEngineProtocolError("local Engine returned malformed JSON") from exc
        finally:
            if temporary_path is not None:
                try:
                    os.unlink(temporary_path)
                except FileNotFoundError:
                    pass
                except OSError:
                    # The request contains only bounded, non-secret selectors;
                    # cleanup failure must not replace the Engine result.
                    pass


def _engine_error_code(stderr: object) -> Optional[str]:
    if not isinstance(stderr, str):
        return None
    line = stderr.strip()
    if not line.startswith("engine: "):
        return None
    code = line.removeprefix("engine: ")
    if not re.fullmatch(r"[a-z][a-z0-9_]{0,63}", code):
        return None
    return code


def _parse_recovery_response(
    payload: object,
    *,
    expected_recovery_ref: str,
    expected_source_ref: str,
    expected_source_digest: str,
) -> RecoveredSource:
    if not isinstance(payload, Mapping):
        raise LeanCtxEngineProtocolError("Engine recovery response must be an object")
    allowed = {
        "schema_version",
        "transport_version",
        "engine_interface_version",
        "view",
        "recovery",
        "invocation",
        "observation",
    }
    unknown = set(payload) - allowed
    if unknown:
        raise LeanCtxEngineProtocolError(f"Engine recovery response contains unknown field {sorted(unknown)[0]}")
    required = ("schema_version", "transport_version", "engine_interface_version", "view", "recovery")
    missing = [field_name for field_name in required if field_name not in payload]
    if missing:
        raise LeanCtxEngineProtocolError(f"Engine recovery response missing {missing[0]}")
    _schema_version(payload["schema_version"], "recovery.schema_version")
    transport_version = _transport_version(payload["transport_version"], "recovery.transport_version")
    engine_interface_version = _version(
        payload["engine_interface_version"], "recovery.engine_interface_version"
    )
    if transport_version != TRANSPORT_VERSION or engine_interface_version != ENGINE_INTERFACE_VERSION:
        raise LeanCtxEngineProtocolError("Engine recovery versions are not pinned")
    recovery = _strict_mapping(
        payload["recovery"],
        "recovery response.recovery",
        required=("recovery_ref", "source_ref", "source_digest"),
    )
    recovery_ref = _ref(recovery["recovery_ref"], "recovery response.recovery_ref")
    source_ref = _ref(recovery["source_ref"], "recovery response.source_ref")
    source_digest = _digest(recovery["source_digest"], "recovery response.source_digest")
    if (recovery_ref, source_ref, source_digest) != (
        expected_recovery_ref,
        expected_source_ref,
        expected_source_digest,
    ):
        raise LeanCtxEngineProtocolError("Engine recovery descriptor does not match admitted source")
    view = _strict_mapping(
        payload["view"],
        "recovery response.view",
        required=("text", "output_ref", "output_digest"),
    )
    text = _bounded_text(view["text"], "recovery view.text")
    output_ref = None if view["output_ref"] is None else _ref(view["output_ref"], "recovery view.output_ref")
    output_digest = _digest(view["output_digest"], "recovery view.output_digest")
    if _sha256_text(text) != expected_source_digest:
        raise LeanCtxEngineProtocolError("recovered source bytes do not match source_digest")
    if output_digest != expected_source_digest:
        raise LeanCtxEngineProtocolError("recovery output_digest does not match source_digest")
    if output_ref is not None and output_ref != "output:" + output_digest.removeprefix("sha256:"):
        raise LeanCtxEngineProtocolError("recovery output_ref does not bind output_digest")
    return RecoveredSource(
        text,
        source_ref=source_ref,
        source_digest=source_digest,
        recovery_ref=recovery_ref,
    )


__all__ = [
    "ContextFailure",
    "ContextMeasurement",
    "ContextPlan",
    "ContextReceiptLink",
    "ContextReceipt",
    "ContextSource",
    "ContextView",
    "ENGINE_BINARY_ENV",
    "ENGINE_INTERFACE_VERSION",
    "EngineExecutionError",
    "EngineProtocolError",
    "EngineRejectedError",
    "EngineTimeoutError",
    "EngineUnavailableError",
    "LocalEngineClient",
    "PREVIEW_VERSION",
    "RecoveredSource",
    "SCHEMA_VERSION",
    "TRANSPORT_VERSION",
    "WIRE_TRANSPORT_VERSION",
]
