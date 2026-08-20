"""Verified Context Kit handles and Runtime Kit loading."""

from __future__ import annotations

import re
import urllib.parse
from dataclasses import dataclass, field
from types import MappingProxyType
from typing import Dict, Mapping, MutableMapping, Optional, Tuple

from .errors import LeanCtxError

_SHA256 = re.compile(r"^[0-9a-f]{64}$")
_OPAQUE = re.compile(r"^[\x21-\x7e]{1,256}$")


def _freeze(value: object) -> object:
    if isinstance(value, Mapping):
        return MappingProxyType({str(key): _freeze(item) for key, item in value.items()})
    if isinstance(value, list):
        return tuple(_freeze(item) for item in value)
    if isinstance(value, tuple):
        return tuple(_freeze(item) for item in value)
    return value


def _opaque(value: object, field_name: str) -> str:
    if not isinstance(value, str) or not _OPAQUE.fullmatch(value):
        raise LeanCtxError(f"invalid Kit {field_name}")
    return value


@dataclass(frozen=True)
class ContextKit:
    """An immutable, version-pinned Context Kit verified by the Runtime."""

    id: str
    version: str
    package_hash: str
    activation_ref: str
    manifest: Mapping[str, object] = field(repr=False, compare=False)

    def __post_init__(self) -> None:
        _opaque(self.id, "id")
        _opaque(self.version, "version")
        _opaque(self.activation_ref, "activation_ref")
        if not isinstance(self.package_hash, str) or not _SHA256.fullmatch(self.package_hash):
            raise LeanCtxError("invalid Kit package_hash")
        if not isinstance(self.manifest, Mapping):
            raise LeanCtxError("invalid Kit manifest")
        object.__setattr__(self, "manifest", _freeze(dict(self.manifest)))


def _parse_kit(value: object) -> ContextKit:
    if not isinstance(value, Mapping):
        raise LeanCtxError("malformed Kit response")
    required = ("id", "version", "package_hash", "activation_ref", "manifest")
    if any(key not in value for key in required):
        raise LeanCtxError("malformed Kit response: required identity field missing")
    manifest = value["manifest"]
    if not isinstance(manifest, Mapping):
        raise LeanCtxError("malformed Kit response: manifest must be an object")
    try:
        kit = ContextKit(
            id=value["id"],
            version=value["version"],
            package_hash=value["package_hash"],
            activation_ref=value["activation_ref"],
            manifest=manifest,
        )
    except (TypeError, ValueError, LeanCtxError) as exc:
        raise LeanCtxError(f"invalid Kit response: {exc}") from exc

    # A Runtime may repeat identity inside its signed manifest, but it may not
    # replace the resolved pin with a mutable alias or a different value.
    for key, expected in (("id", kit.id), ("version", kit.version), ("package_hash", kit.package_hash)):
        if key in manifest and manifest[key] != expected:
            raise LeanCtxError("Kit manifest identity does not match resolved pin")
    return kit


def load_kit(
    name: object,
    *,
    proxy: object,
    cache: MutableMapping[Tuple[str, str, str], ContextKit],
    timeout: float,
) -> ContextKit:
    """Resolve ``name`` once through the authenticated Runtime Kit endpoint."""
    del timeout  # The reusable ProxyClient owns the configured transport timeout.
    if isinstance(name, ContextKit):
        return name
    if not isinstance(name, str) or not name.strip():
        raise ValueError("Kit name must be a non-empty string")
    quoted = urllib.parse.quote(name, safe="")
    try:
        response = proxy._get_response(f"/v1/kits/{quoted}")
    except AttributeError as exc:  # pragma: no cover - defensive private bridge guard
        raise LeanCtxError("invalid proxy client for Kit loading") from exc
    body = response[0] if isinstance(response, tuple) else response
    kit = _parse_kit(body)
    key = (kit.id, kit.version, kit.package_hash)
    cached = cache.get(key)
    if cached is not None:
        return cached
    cache[key] = kit
    return kit


parse_kit = _parse_kit
