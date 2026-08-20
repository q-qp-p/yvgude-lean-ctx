"""Compatibility proxy client plus the private v1 session transport bridge."""

from __future__ import annotations

import json
import re
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from types import MappingProxyType
from typing import Any, Dict, List, Mapping, Optional, Tuple

from . import discovery
from .errors import LeanCtxAuthError, LeanCtxConnectionError, LeanCtxError

Message = Dict[str, Any]
_DEFAULT_TIMEOUT = 30.0
_SHA256 = re.compile(r"^sha256:[0-9a-f]{64}$")
_OPAQUE = re.compile(r"^[\x21-\x7e]{1,256}$")
_DECIMAL = re.compile(r"^[0-9]+$")
_COVERAGE = {
    "observed",
    "compressed",
    "context_controlled",
    "full_inline",
    "not_addressable",
}


@dataclass(frozen=True)
class ProxyHTTPResponse:
    """Raw proxy body with normalized, immutable response headers."""

    body: bytes
    headers: Mapping[str, str]
    status: int

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            "headers",
            MappingProxyType({str(key).lower(): str(value) for key, value in self.headers.items()}),
        )


@dataclass(frozen=True)
class ProxyObservation:
    """Validated request-scoped evidence returned by a bound proxy request."""

    request_id: Optional[str]
    execution_receipt_id: Optional[str]
    canonical_hash: Optional[str]
    usage: Mapping[str, Optional[int]]
    coverage: str
    provider: Optional[str]
    model: Optional[str]
    latency_ms: Optional[int]

    def __post_init__(self) -> None:
        object.__setattr__(self, "usage", MappingProxyType(dict(self.usage)))


@dataclass
class CompressResult:
    """Result of a ``/v1/compress`` call: rewritten messages plus savings."""

    messages: List[Message]
    stats: Dict[str, Any] = field(default_factory=dict)

    @property
    def original_tokens(self) -> int:
        return int(self.stats.get("original_tokens", 0))

    @property
    def compressed_tokens(self) -> int:
        return int(self.stats.get("compressed_tokens", 0))

    @property
    def saved_tokens(self) -> int:
        return int(self.stats.get("saved_tokens", 0))

    @property
    def saved_pct(self) -> float:
        return float(self.stats.get("saved_pct", 0.0))


def _opaque_header(value: object, name: str) -> str:
    if not isinstance(value, str) or value != value.strip() or not _OPAQUE.fullmatch(value):
        raise LeanCtxError(f"invalid {name} header")
    return value


def _number_header(value: object, name: str) -> Optional[int]:
    if value == "unknown":
        return None
    if not isinstance(value, str) or not _DECIMAL.fullmatch(value):
        raise LeanCtxError(f"invalid {name} header")
    return int(value)


def _identifier_header(value: object, name: str) -> Optional[str]:
    if value == "unknown":
        return None
    return _opaque_header(value, name)


def _unobserved() -> ProxyObservation:
    return ProxyObservation(
        request_id=None,
        execution_receipt_id=None,
        canonical_hash=None,
        usage={
            "input_tokens": None,
            "output_tokens": None,
            "cached_tokens": None,
            "reasoning_tokens": None,
            "compression_original_tokens": None,
            "compression_delivered_tokens": None,
        },
        coverage="not_addressable",
        provider=None,
        model=None,
        latency_ms=None,
    )


def parse_proxy_observation(response: ProxyHTTPResponse) -> ProxyObservation:
    """Parse the v1 response-header evidence without inventing unknown values."""
    headers = response.headers
    prefix = "x-leanctx-"
    relevant = {key: value for key, value in headers.items() if key.startswith(prefix)}
    if not relevant:
        return _unobserved()

    protocol = headers.get("x-leanctx-protocol")
    if protocol is not None and protocol != "1":
        raise LeanCtxError("invalid X-LeanCTX-Protocol response header")

    # Validate all present known-value headers before deciding whether this call
    # was attributable. A partial response may be unobserved, but it may not
    # smuggle malformed telemetry through that path.
    usage_headers = {
        "input_tokens": "x-leanctx-input-tokens",
        "output_tokens": "x-leanctx-output-tokens",
        "cached_tokens": "x-leanctx-cached-tokens",
        "reasoning_tokens": "x-leanctx-reasoning-tokens",
        "compression_original_tokens": "x-leanctx-compression-original-tokens",
        "compression_delivered_tokens": "x-leanctx-compression-delivered-tokens",
    }
    parsed_usage: Dict[str, Optional[int]] = {}
    for name, header in usage_headers.items():
        raw = headers.get(header)
        parsed_usage[name] = None if raw is None else _number_header(raw, header)

    latency_raw = headers.get("x-leanctx-latency-ms")
    latency = None if latency_raw is None else _number_header(latency_raw, "x-leanctx-latency-ms")
    provider_raw = headers.get("x-leanctx-provider")
    provider = None if provider_raw is None else _identifier_header(provider_raw, "x-leanctx-provider")
    model_raw = headers.get("x-leanctx-model")
    model = None if model_raw is None else _identifier_header(model_raw, "x-leanctx-model")

    request_raw = headers.get("x-leanctx-request-id")
    execution_raw = headers.get("x-leanctx-execution-receipt-id")
    hash_raw = headers.get("x-leanctx-canonical-hash")
    coverage_raw = headers.get("x-leanctx-coverage")
    if request_raw is not None:
        _opaque_header(request_raw, "x-leanctx-request-id")
    if execution_raw is not None:
        _opaque_header(execution_raw, "x-leanctx-execution-receipt-id")
    if hash_raw is not None and (not isinstance(hash_raw, str) or not _SHA256.fullmatch(hash_raw)):
        raise LeanCtxError("invalid x-leanctx-canonical-hash header")
    if coverage_raw is not None and coverage_raw not in _COVERAGE:
        raise LeanCtxError("invalid x-leanctx-coverage header")
    if any(value is None for value in (protocol, request_raw, execution_raw, hash_raw, coverage_raw)):
        return _unobserved()
    request_id = _opaque_header(request_raw, "x-leanctx-request-id")
    execution_receipt_id = _opaque_header(execution_raw, "x-leanctx-execution-receipt-id")
    if not isinstance(hash_raw, str) or not _SHA256.fullmatch(hash_raw):
        raise LeanCtxError("invalid x-leanctx-canonical-hash header")
    if coverage_raw not in _COVERAGE:
        raise LeanCtxError("invalid x-leanctx-coverage header")
    return ProxyObservation(
        request_id=request_id,
        execution_receipt_id=execution_receipt_id,
        canonical_hash=hash_raw,
        usage=parsed_usage,
        coverage=coverage_raw,
        provider=provider,
        model=model,
        latency_ms=latency,
    )


class ProxyClient:
    """Reusable client for local, deterministic lean-ctx proxy endpoints."""

    def __init__(
        self,
        base_url: Optional[str] = None,
        token: Optional[str] = None,
        timeout: float = _DEFAULT_TIMEOUT,
    ) -> None:
        self.base_url = discovery.resolve_base_url(base_url)
        self.token = discovery.resolve_token(token)
        self.timeout = timeout

    def compress(
        self,
        messages: List[Message],
        model: Optional[str] = None,
    ) -> CompressResult:
        """Compress unbound legacy traffic without adding session headers."""
        if not isinstance(messages, list):
            raise TypeError("messages must be a list of chat-message dicts")
        return self._compress(messages, model, headers=None)

    def compress_bound(
        self,
        messages: List[Message],
        model: Optional[str],
        session_headers: Mapping[str, str],
    ) -> Tuple[CompressResult, ProxyObservation]:
        """Compress through one established session and return response evidence."""
        if not isinstance(messages, list):
            raise TypeError("messages must be a list of chat-message dicts")
        if not isinstance(session_headers, Mapping):
            raise TypeError("session_headers must be a mapping")
        result, response = self._compress(messages, model, headers=session_headers, with_response=True)
        return result, parse_proxy_observation(response)

    def _compress(
        self,
        messages: List[Message],
        model: Optional[str],
        headers: Optional[Mapping[str, str]],
        with_response: bool = False,
    ) -> Any:
        payload: Dict[str, Any] = {"messages": messages}
        if model:
            payload["model"] = model
        data, response = self._post_response("/v1/compress", payload, headers=headers)
        out = data.get("messages")
        if not isinstance(out, list):
            raise LeanCtxError("malformed /v1/compress response: 'messages' missing")
        stats = data.get("stats")
        result = CompressResult(messages=out, stats=stats if isinstance(stats, dict) else {})
        return (result, response) if with_response else result

    def resolve_reference(self, reference_id: str) -> str:
        """Return the original content behind a URL-quoted reference identifier."""
        if not reference_id:
            raise ValueError("reference_id must be a non-empty string")
        quoted = urllib.parse.quote(reference_id, safe="")
        request = self._request(f"/v1/references/{quoted}", method="GET")
        return self._send(request).body.decode("utf-8")

    def _request(
        self,
        path: str,
        *,
        method: str,
        data: Optional[bytes] = None,
        headers: Optional[Mapping[str, str]] = None,
    ) -> urllib.request.Request:
        request = urllib.request.Request(f"{self.base_url}{path}", data=data, method=method)
        if data is not None:
            request.add_header("Content-Type", "application/json")
        if self.token:
            request.add_header("Authorization", f"Bearer {self.token}")
        if headers:
            for name, value in headers.items():
                request.add_header(str(name), str(value))
        return request

    def _post(self, path: str, payload: Dict[str, Any]) -> Dict[str, Any]:
        return self._post_response(path, payload)[0]

    def _post_response(
        self,
        path: str,
        payload: Dict[str, Any],
        headers: Optional[Mapping[str, str]] = None,
    ) -> Tuple[Dict[str, Any], ProxyHTTPResponse]:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        request = self._request(path, method="POST", data=body, headers=headers)
        response = self._send(request)
        return self._json(response, request.full_url), response

    def _get_response(self, path: str) -> Tuple[Dict[str, Any], ProxyHTTPResponse]:
        request = self._request(path, method="GET")
        response = self._send(request)
        return self._json(response, request.full_url), response

    @staticmethod
    def _json(response: ProxyHTTPResponse, url: str) -> Dict[str, Any]:
        try:
            data = json.loads(response.body.decode("utf-8"))
        except (ValueError, TypeError, UnicodeDecodeError) as exc:
            raise LeanCtxError(f"invalid JSON response from {url}: {exc}") from exc
        if not isinstance(data, dict):
            raise LeanCtxError(f"invalid JSON response from {url}: expected object")
        return data

    def _send(self, request: urllib.request.Request) -> ProxyHTTPResponse:
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return ProxyHTTPResponse(
                    body=response.read(),
                    headers=dict(response.headers.items()),
                    status=getattr(response, "status", response.getcode()),
                )
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace").strip()
            if exc.code in (401, 403):
                raise LeanCtxAuthError(
                    f"proxy rejected the request (HTTP {exc.code}). "
                    "Set LEAN_CTX_PROXY_TOKEN or pass token=…"
                ) from exc
            if exc.code == 404:
                raise LeanCtxError(f"{request.full_url} not found (HTTP 404): {detail}") from exc
            if 500 <= exc.code <= 599:
                raise LeanCtxConnectionError(
                    f"lean-ctx proxy service failure (HTTP {exc.code}) at {request.full_url}"
                ) from exc
            raise LeanCtxError(
                f"{request.get_method()} {request.full_url} failed (HTTP {exc.code}): {detail}"
            ) from exc
        except urllib.error.URLError as exc:
            raise LeanCtxConnectionError(
                f"could not reach the lean-ctx proxy at {self.base_url} ({exc.reason}). "
                "Is the daemon running? Try: lean-ctx proxy enable"
            ) from exc


def compress(
    messages: List[Message],
    model: Optional[str] = None,
    *,
    base_url: Optional[str] = None,
    token: Optional[str] = None,
    timeout: float = _DEFAULT_TIMEOUT,
) -> List[Message]:
    """Compress a legacy chat message list, returning only rewritten messages."""
    client = ProxyClient(base_url=base_url, token=token, timeout=timeout)
    return client.compress(messages, model=model).messages
