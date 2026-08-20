"""Live adapter smoke tests against a real lean-ctx server (GL #395).

One end-to-end test per framework adapter (OpenAI / LangChain / LlamaIndex /
CrewAI): build the tools from the live ``/v1/tools`` manifest, execute one real
tool call through the framework's own invocation path, and assert real output.

Driven by ``scripts/sdk-conformance.sh`` (CI job ``sdk-conformance``) via
``LEANCTX_CONFORMANCE_URL``. Without the URL the suite skips (hermetic local
``pytest``); without the optional framework the individual test skips.
"""

from __future__ import annotations

import os

import pytest

from leanctx import LeanCtxClient
from leanctx.adapters import (
    run_openai_tool_call,
    to_crewai_tools,
    to_langchain_tools,
    to_llamaindex_tools,
    to_openai_tools,
)
from leanctx.adapters._common import normalized_tool_specs

URL = os.environ.get("LEANCTX_CONFORMANCE_URL", "").strip()

pytestmark = pytest.mark.skipif(not URL, reason="LEANCTX_CONFORMANCE_URL not set")


@pytest.fixture(scope="module")
def client() -> LeanCtxClient:
    return LeanCtxClient(
        URL, bearer_token=os.environ.get("LEANCTX_CONFORMANCE_TOKEN") or None
    )


@pytest.fixture(scope="module")
def smoke_tool(client: LeanCtxClient) -> str:
    """Pick a real tool that needs no arguments, straight from the live manifest."""
    specs = normalized_tool_specs(client)
    assert specs, "live server returned no tools"
    no_arg = [s.name for s in specs if not s.parameters.get("required")]
    assert no_arg, "no argument-free tool available for the smoke call"
    # Prefer cheap, read-only diagnostics when present.
    for preferred in ("ctx_health", "ctx_metrics", "ctx_overview"):
        if preferred in no_arg:
            return preferred
    return no_arg[0]


def _pick(items: list, name_of, name: str) -> object:
    matches = [t for t in items if name_of(t) == name]
    assert matches, f"{name} missing from adapter output"
    return matches[0]


def test_openai_adapter_live_round_trip(client: LeanCtxClient, smoke_tool: str) -> None:
    specs = to_openai_tools(client)
    assert specs and all(s["type"] == "function" for s in specs)
    spec = _pick(specs, lambda s: s["function"]["name"], smoke_tool)

    # The exact dict shape an OpenAI chat completion returns for a tool call.
    out = run_openai_tool_call(
        client,
        {"function": {"name": spec["function"]["name"], "arguments": "{}"}},
    )
    assert isinstance(out, str) and out.strip(), "empty tool output"


def test_langchain_adapter_live_round_trip(
    client: LeanCtxClient, smoke_tool: str
) -> None:
    pytest.importorskip("langchain_core")
    tool = _pick(to_langchain_tools(client), lambda t: t.name, smoke_tool)
    out = tool.invoke("{}")
    assert isinstance(out, str) and out.strip()


def test_llamaindex_adapter_live_round_trip(
    client: LeanCtxClient, smoke_tool: str
) -> None:
    pytest.importorskip("llama_index.core")
    tool = _pick(
        to_llamaindex_tools(client), lambda t: t.metadata.name, smoke_tool
    )
    out = tool.call("{}")
    assert str(out).strip()


def test_crewai_adapter_live_round_trip(
    client: LeanCtxClient, smoke_tool: str
) -> None:
    pytest.importorskip("crewai")
    tool = _pick(to_crewai_tools(client), lambda t: t.name, smoke_tool)
    out = tool.run(arguments="{}")
    assert str(out).strip()


def test_adapter_specs_cover_full_manifest(client: LeanCtxClient) -> None:
    """Drift gate: every tool in the live manifest converts to an OpenAI spec."""
    manifest_names = {s.name for s in normalized_tool_specs(client)}
    spec_names = {s["function"]["name"] for s in to_openai_tools(client)}
    assert spec_names == manifest_names, (
        f"adapter dropped tools: {sorted(manifest_names - spec_names)}"
    )
