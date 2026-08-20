"""Pydantic models for OCLA Wire API responses."""

from __future__ import annotations

from typing import Any, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field


OCLA_API_VERSION = "ocla/v1"
U64_MAX = 2**64 - 1


class WireModel(BaseModel):
    """Base model that rejects fields outside the public wire contract."""

    model_config = ConfigDict(extra="forbid")


class HealthResponse(WireModel):
    """Response from ``GET /ocla/v1/health``."""

    status: Literal["ok"]
    version: Literal["ocla/v1"]


class ErrorResponse(WireModel):
    """Error body returned when an OCLA request is rejected."""

    error: str
    code: Optional[str] = None


class OclaRequestContext(WireModel):
    """Lineage identifiers carried by a token envelope."""

    request_id: str
    session_id: str
    agent_id: str
    content_ref: str
    tenant_id: Optional[str]


class TokenBalance(WireModel):
    """Token counts recorded at each OCLA lifecycle stage."""

    original_tokens: int = Field(ge=0, le=U64_MAX)
    materialized_tokens: int = Field(ge=0, le=U64_MAX)
    delivered_tokens: int = Field(ge=0, le=U64_MAX)
    provider_billed_tokens: int = Field(ge=0, le=U64_MAX)


class MessageV1(WireModel):
    """One message captured in a canonical envelope payload."""

    role: Literal["system", "user", "assistant", "tool"]
    content: str | list[dict[str, Any]]
    name: str | None = None


class MessagesPayload(WireModel):
    """Envelope payload containing a complete message list."""

    type: Literal["messages"] = "messages"
    messages: list[MessageV1]


class StreamChunkPayload(WireModel):
    """Envelope payload containing one streamed output chunk."""

    type: Literal["stream_chunk"] = "stream_chunk"
    chunk_index: int
    delta: str
    finish_reason: str | None = None


class ToolCallPayload(WireModel):
    """Envelope payload containing a tool invocation and optional result."""

    type: Literal["tool_call"] = "tool_call"
    tool_name: str
    arguments: str
    result: str | None = None


class UsagePayload(WireModel):
    """Envelope payload containing provider usage costs."""

    type: Literal["usage"] = "usage"
    input_cost_usd: float | None = None
    output_cost_usd: float | None = None
    total_cost_usd: float | None = None
    currency: str = "USD"


EnvelopePayload = MessagesPayload | StreamChunkPayload | ToolCallPayload | UsagePayload


class QualitySignal(WireModel):
    fidelity: float | None = None
    calibration_accuracy: float | None = None
    compression_ratio: float | None = None


class PlanBudget(WireModel):
    max_tokens: int | None = None
    reserved_tokens: int | None = None
    priority: str | None = None


class PlanEntry(WireModel):
    path: str
    mode: str
    tokens: int
    score: float | None = None
    reason: str | None = None


class ExcludedEntry(WireModel):
    path: str
    reason: str


class ContextPlanV1(WireModel):
    plan_id: str
    timestamp: str
    entries: list[PlanEntry] = Field(default_factory=list)
    excluded: list[ExcludedEntry] = Field(default_factory=list)
    budget: PlanBudget | None = None
    policy: dict[str, Any] | None = None


class ContextReceiptV1(WireModel):
    receipt_id: str
    plan_id: str | None = None
    timestamp: str
    delivered_tokens: int
    saved_tokens: int
    outcome: str | None = None
    quality: QualitySignal | None = None


class AttributionEntry(WireModel):
    source: str
    original_tokens: int
    delivered_tokens: int
    savings_pct: float | None = None


class AttributionReport(WireModel):
    entries: list[AttributionEntry] = Field(default_factory=list)
    total_original: int = 0
    total_delivered: int = 0


class EnvelopeResponse(WireModel):
    """Validated canonical token envelope returned by the envelope endpoint."""

    schema_version: Literal[1]
    context: OclaRequestContext
    surface: Literal["mcp", "proxy", "shell", "agent"]
    direction: Literal["input", "output"]
    provider: str
    model: str
    token_balance: TokenBalance
    route_ref: Optional[str] = None
    policy_ref: Optional[str] = None
    idempotency_key: str
    payload: EnvelopePayload | None = None


class Capability(WireModel):
    """One registered OCLA capability."""

    kind: Literal[
        "observation_hook",
        "usage_sink",
        "metrics_exporter",
        "savings_ledger",
        "intent_classifier",
        "outcome_tracker",
        "compression_provider",
        "response_optimizer",
        "model_router",
        "efficiency_analyzer",
        "config_tuner",
        "experiment_runner",
        "connector_scheduler",
        "agent_gateway",
    ]
    api_version: Literal["ocla/v1"]
    status: Literal["available", "degraded", "unavailable"]
    limits: dict[str, int]


class CapabilitiesResponse(WireModel):
    """Response from ``GET /ocla/v1/capabilities``."""

    version: Literal["ocla/v1"]
    capabilities: list[Capability]


class LedgerSummary(WireModel):
    """Response from ``GET /ocla/v1/ledger/summary``."""

    events: int = Field(ge=0)
    tokens: int = Field(ge=0, le=U64_MAX)
    usd: float
