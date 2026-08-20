from __future__ import annotations

import asyncio
import json
from unittest.mock import AsyncMock, Mock

import httpx
from pydantic import TypeAdapter

from lean_ctx import (
    MessageV1,
    MessagesPayload,
    OclaClient,
    StreamChunkPayload,
    ToolCallPayload,
    UsagePayload,
)
from lean_ctx.models import EnvelopePayload
from lean_ctx.streaming import stream_events


def run(coroutine):
    return asyncio.run(coroutine)


def client_for(
    routes: dict[tuple[str, str], dict],
) -> tuple[OclaClient, httpx.AsyncClient]:
    def handler(request: httpx.Request) -> httpx.Response:
        key = (request.method, request.url.path)
        payload = routes[key]
        if request.method == "POST":
            assert json.loads(request.content) == payload["request"]
            return httpx.Response(200, json=payload["response"])
        return httpx.Response(200, json=payload)

    transport = httpx.MockTransport(handler)
    http_client = httpx.AsyncClient(transport=transport)
    return OclaClient("https://ocla.test", client=http_client), http_client


def test_health_and_capabilities() -> None:
    routes = {
        ("GET", "/ocla/v1/health"): {"status": "ok", "version": "ocla/v1"},
        ("GET", "/ocla/v1/capabilities"): {
            "version": "ocla/v1",
            "capabilities": [
                {
                    "kind": "agent_gateway",
                    "api_version": "ocla/v1",
                    "status": "available",
                    "limits": {"max_input_tokens": 4096},
                }
            ],
        },
    }
    sdk, http_client = client_for(routes)

    async def scenario() -> None:
        health = await sdk.health()
        capabilities = await sdk.capabilities()
        assert health.status == "ok"
        assert health.version == "ocla/v1"
        assert capabilities.capabilities[0].kind == "agent_gateway"
        assert capabilities.capabilities[0].limits == {"max_input_tokens": 4096}
        await http_client.aclose()

    run(scenario())


def test_validate_envelope_and_ledger_summary() -> None:
    envelope = {
        "schema_version": 1,
        "context": {
            "request_id": "request-1",
            "session_id": "session-1",
            "agent_id": "agent-1",
            "content_ref": "blake3:content",
            "tenant_id": None,
        },
        "surface": "proxy",
        "direction": "input",
        "provider": "openai",
        "model": "gpt-5",
        "token_balance": {
            "original_tokens": 100,
            "materialized_tokens": 80,
            "delivered_tokens": 60,
            "provider_billed_tokens": 60,
        },
        "route_ref": "route-1",
        "policy_ref": None,
        "idempotency_key": "request-1:input",
        "payload": {
            "type": "messages",
            "messages": [{"role": "user", "content": "hello"}],
        },
    }
    routes = {
        ("POST", "/ocla/v1/envelope"): {
            "request": envelope,
            "response": envelope,
        },
        ("GET", "/ocla/v1/ledger/summary"): {
            "events": 3,
            "tokens": 120,
            "usd": 0.42,
        },
    }
    sdk, http_client = client_for(routes)

    async def scenario() -> None:
        response = await sdk.validate_envelope(envelope)
        summary = await sdk.ledger_summary()
        assert response.context.request_id == "request-1"
        assert response.token_balance.delivered_tokens == 60
        assert isinstance(response.payload, MessagesPayload)
        assert summary.events == 3
        assert summary.tokens == 120
        assert summary.usd == 0.42
        await http_client.aclose()

    run(scenario())


def test_capsule_endpoints() -> None:
    def response(payload: dict) -> Mock:
        mocked = Mock()
        mocked.json.return_value = payload
        return mocked

    http_client = Mock()
    http_client.post = AsyncMock(
        side_effect=[
            response({"capsule_ref": "capsule:1"}),
            response({"capsule_ref": "capsule:2"}),
        ]
    )
    http_client.get = AsyncMock(
        return_value=response(
            {"capsule_ref": "capsule:1", "data": "capsule data"}
        )
    )
    sdk = OclaClient("https://ocla.test", client=http_client)

    async def scenario() -> None:
        registered = await sdk.register_capsule("capsule data")
        resolved = await sdk.resolve_capsule(registered)
        forked = await sdk.fork_capsule(registered, 1000)

        assert registered == "capsule:1"
        assert resolved == {"capsule_ref": "capsule:1", "data": "capsule data"}
        assert forked == "capsule:2"
        http_client.post.assert_any_await(
            "https://ocla.test/ocla/v1/capsule", content="capsule data"
        )
        http_client.post.assert_any_await(
            "https://ocla.test/ocla/v1/capsule/capsule:1/fork",
            json={"budget_tokens": 1000},
        )
        http_client.get.assert_awaited_once_with(
            "https://ocla.test/ocla/v1/capsule/capsule:1"
        )

    run(scenario())


def test_envelope_payload_models_validate_and_round_trip() -> None:
    payloads = [
        MessagesPayload(messages=[MessageV1(role="user", content="hello")]),
        StreamChunkPayload(chunk_index=0, delta="hel", finish_reason=None),
        ToolCallPayload(tool_name="ctx_read", arguments='{"path":"README.md"}'),
        UsagePayload(input_cost_usd=0.01, output_cost_usd=0.02, total_cost_usd=0.03),
    ]
    adapter = TypeAdapter(EnvelopePayload)

    for payload in payloads:
        restored = adapter.validate_json(payload.model_dump_json())
        assert restored == payload


def test_stream_events_parses_multiple_events() -> None:
    async def scenario() -> None:
        body = (
            'event: envelope\n'
            'data: {"schema_version":1}\n\n'
            'event: heartbeat\n'
            'data: {"alive":true}\n\n'
            'event: envelope\n'
            'data: {"schema_version":1,"payload":{"type":"stream_chunk"}}\n\n'
        )
        response = httpx.Response(200, content=body)
        events = [event async for event in stream_events(response)]
        assert events == [
            {"type": "envelope", "data": {"schema_version": 1}},
            {"type": "heartbeat", "data": {"alive": True}},
            {
                "type": "envelope",
                "data": {
                    "schema_version": 1,
                    "payload": {"type": "stream_chunk"},
                },
            },
        ]

    run(scenario())


def test_stream_envelopes_yields_envelope_events_only() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.headers["accept"] == "text/event-stream"
        return httpx.Response(
            200,
            content=(
                'event: heartbeat\n'
                'data: {"alive":true}\n\n'
                'event: envelope\n'
                'data: {"schema_version":1}\n\n'
            ),
        )

    async def scenario() -> None:
        http_client = httpx.AsyncClient(transport=httpx.MockTransport(handler))
        sdk = OclaClient("https://ocla.test", client=http_client)
        events = [event async for event in sdk.stream_envelopes()]
        assert events == [{"schema_version": 1}]
        await http_client.aclose()

    run(scenario())
