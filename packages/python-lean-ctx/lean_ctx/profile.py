"""Immutable, Runtime-pinned tuning profiles."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


def _required(value: object, field_name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{field_name} must be a non-empty string")
    return value


@dataclass(frozen=True)
class TuningProfile:
    """A resolved profile identity returned by the Runtime."""

    id: str
    version: str
    content_hash: str
    source_ref: str
    context_budget: Optional[int] = None
    compression_mode: Optional[str] = None
    recovery_policy: Optional[str] = None

    def __post_init__(self) -> None:
        _required(self.id, "profile id")
        _required(self.version, "profile version")
        _required(self.content_hash, "profile content_hash")
        _required(self.source_ref, "profile source_ref")
        if self.context_budget is not None and (
            not isinstance(self.context_budget, int) or self.context_budget < 0
        ):
            raise ValueError("context_budget must be a non-negative integer or None")


def parse_profile(value: object) -> TuningProfile:
    """Parse a Runtime profile pin without accepting partial identities."""
    if not isinstance(value, dict):
        raise ValueError("Runtime profile must be an object")
    try:
        return TuningProfile(
            id=value["id"],
            version=value["version"],
            content_hash=value["content_hash"],
            source_ref=value["source_ref"],
            context_budget=value.get("context_budget"),
            compression_mode=value.get("compression_mode"),
            recovery_policy=value.get("recovery_policy"),
        )
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError(f"invalid Runtime profile pin: {exc}") from exc
