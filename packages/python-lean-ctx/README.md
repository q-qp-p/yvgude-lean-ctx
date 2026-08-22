# lean-ctx-python — Preview

Python SDK for [lean-ctx](https://github.com/yvgude/lean-ctx) — context compression and execution evidence for AI agents.

[![PyPI](https://img.shields.io/pypi/v/lean-ctx-python)](https://pypi.org/project/lean-ctx-python/)
[![Python](https://img.shields.io/pypi/pyversions/lean-ctx-python)](https://pypi.org/project/lean-ctx-python/)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Requires Python 3.9+. Zero runtime dependencies.

**Product status: Preview.** This is LeanCTX's declared Python SDK v1 and
reference-wrapper scope, not a general multi-framework performance guarantee.
Receipt fields reflect only Runtime-observed evidence; unavailable sealing or
transport observation is recorded as an incomplete/degraded result, never
invented as verified savings.

## Quick Start

Install the SDK and ensure the local lean-ctx proxy is running (`lean-ctx proxy enable`).

```python
from lean_ctx import LeanCTX

ctx = LeanCTX()
wrapped = ctx.wrap(my_agent)
run = wrapped.run("Review the payments module")
print(run.output)
print(run.receipt.verify())
print(run.receipt.savings.saved_tokens)
```

Each `run()` returns the agent's original output plus an `ExecutionReceipt`.
It is sealed and independently verifiable only when the Runtime supplies the
required evidence; otherwise the receipt remains explicitly incomplete or
degraded.

## Installation

```bash
pip install lean-ctx-python
```

Optional extras (install only what you need):

```bash
pip install "lean-ctx-python[langchain]"    # LangChain message compression + retriever
pip install "lean-ctx-python[litellm]"      # LiteLLM pre-call hook
pip install "lean-ctx-python[llamaindex]"   # LlamaIndex node parser
pip install "lean-ctx-python[openai-agents]" # OpenAI Agents SDK adapter (Python 3.10+)
pip install "lean-ctx-python[verify]"       # Ed25519 receipt signature verification (cryptography>=42.0)
pip install "lean-ctx-python[all]"          # all optional integrations + verify
pip install "lean-ctx-python[test]"         # pytest (development only)
```

The SDK communicates with a local lean-ctx proxy over HTTP. The wrap/session API expects the Runtime proxy at `http://localhost:8077` by default. The legacy `compress()` API discovers the proxy port automatically (see [Configuration](#configuration)).

## How it works

```
  Your agent                Python SDK                 lean-ctx proxy (local)
  ──────────                ──────────                 ──────────────────────
  agent.run(task)  ──►  LeanCTX.wrap() / session  ──►  /v1/sessions, /v1/compress
                              │                              │
                              │◄── session headers ──────────┤
                              │                              │
  LLM calls ─────────►  transport.compress()  ──►  compress + observe headers
                              │                              │
                              ▼                              ▼
                        ExecutionReceipt  ◄────  seal + sign canonical payload
```

1. **`LeanCTX.wrap(agent)`** selects an adapter based on your agent's `run` signature and binds a task-scoped `ContextSession`.
2. During the run, LLM traffic routed through the bound transport is compressed by the proxy. Response headers carry usage and execution-receipt identifiers.
3. When the agent finishes, the SDK seals a session receipt via the proxy. The result is an immutable `ExecutionReceipt` you can verify locally or against the Runtime verifier.

The SDK does not call cloud LLM APIs directly. It orchestrates sessions, forwards compression to the local proxy, and parses the evidence the Runtime returns.

## Core Concepts

**Wrap.** `LeanCTX.wrap(agent)` returns a `WrappedAgent` with a `.run(task)` method. Each call opens a fresh Runtime session, runs your agent once, and returns a `LeanCtxRun`. Your agent's return value is unchanged; evidence is attached separately on `run.receipt`.

**Receipt.** An `ExecutionReceipt` is the Runtime's signed record of one wrapped run: task identity, profile and kit pins, coverage, outcome, degradations, and savings. Receipts are parsed from the proxy response — the SDK does not invent token or cost numbers locally.

**Verify.** `receipt.verify()` returns `True` only on affirmative proof: the receipt must be sealed, the canonical SHA-256 digest must match, and either an Ed25519 signature verifies (when `cryptography` is installed and a public key is present) or the Runtime `/v1/receipts/{id}/verify` endpoint confirms it. Any uncertainty returns `False`.

## API Reference

| Type | Role |
|------|------|
| `LeanCTX` | Facade: `wrap()`, `session()`, `load_kit()` |
| `LeanCTXConfig` | Frozen configuration (project, proxy, timeout, profiles, fail-open) |
| `WrappedAgent` | Agent wrapper with `.run(task) -> LeanCtxRun` |
| `LeanCtxRun` | `output`, `receipt`, `metrics` for one completed run |
| `ExecutionReceipt` | Immutable evidence: `verify()`, `savings`, IDs, degradations |
| `SavingsInfo` | Token and cost fields issued by the Runtime (`receipt.savings`) |
| `ContextSession` | Task-scoped session (created internally; use via `ctx.session()`) |
| `ContextKit` | Immutable, Runtime-verified kit handle from `load_kit()` |
| `TuningProfile` | Resolved profile pin returned in receipts |
| `ProxyClient` | Low-level HTTP client for `/v1/compress` and verifier endpoints |
| `compress()` | Legacy one-shot message compression (no session) |
| `LeanCtxClient` | Thin wrapper around the `lean-ctx` CLI (`read`, `search`, `shell`) |

Framework helpers: `compress_messages`, `LeanCtxRetriever`, `compress_request_data`, `LeanCtxLiteLLMHandler`, `LeanCtxNodeParser`.

## Agent Adapters

The SDK inspects your agent's `run` method and selects exactly one adapter. Agents must define a callable `run` that accepts a task positional argument.

### 1. RunOnlyAgent (degraded)

Agent has `run(task)` only. No proxy binding is injected. The agent still executes, but compression evidence is limited unless the agent makes LLM calls through other means. With `fail_open=True` (default), the run proceeds and the receipt records a degradation.

```python
class MyAgent:
    def run(self, task: str) -> str:
        return do_work(task)
```

### 2. ContextAwareAgent

Agent accepts an optional keyword argument `lean-ctx-python`. When the proxy session is available, the SDK passes a `RunTransport` with bound `compress()` and observation recording.

```python
class MyAgent:
    def run(self, task: str, *, lean-ctx-python=None) -> str:
        if lean-ctx-python is not None:
            result = lean-ctx-python.compress(messages, model="gpt-4o")
            messages = result.messages
        return call_llm(messages)
```

When the proxy is unavailable and `fail_open=True`, the SDK calls `run(task, lean-ctx-python=None)` so the agent can detect missing transport explicitly.

### 3. ProxyBoundAgent

Agent implements `set_lean-ctx-python_transport(proxy, headers)` (returning an optional reset callable). The SDK configures transport before `run(task)`.

```python
class MyAgent:
    def set_lean-ctx-python_transport(self, *, proxy, headers):
        self._proxy = proxy
        self._headers = headers
        return lambda: setattr(self, "_proxy", None)

    def run(self, task: str) -> str:
        return self._call_via_proxy(task)
```

An agent matching both `set_lean-ctx-python_transport` and a `lean-ctx-python` keyword raises `LeanCtxError` at wrap time.

## Configuration

### `LeanCTXConfig` fields

| Field | Default | Description |
|-------|---------|-------------|
| `project` | `None` | Project identifier sent to the Runtime |
| `agent_id` | `None` | Opaque agent ID (auto-derived from class name if unset) |
| `proxy_url` | `None` | Proxy base URL; wrap API defaults to `http://localhost:8077` |
| `proxy_token` | `None` | Bearer token for proxy auth |
| `timeout` | `30.0` | HTTP timeout in seconds |
| `default_profile` | `"balanced"` | Profile pin for wrapped runs |
| `fail_open` | `True` | Degrade gracefully when the proxy is unreachable |
| `integration_depth` | `"wrap"` | `"attach"`, `"wrap"`, or `"embed"` (`"embed"` raises in Python v1) |

Pass a mapping or `LeanCTXConfig` to the constructor:

```python
from lean_ctx import LeanCTX, LeanCTXConfig

ctx = LeanCTX(LeanCTXConfig(project="my-project", default_profile="balanced"))
ctx = LeanCTX({"proxy_url": "http://localhost:8077", "project": "my-project"})
```

Wrap-time overrides:

```python
wrapped = ctx.wrap(agent, kit="payments", profile="aggressive")
```

### Environment variables and discovery

Used by `ProxyClient` and the legacy `compress()` API (same rules as the lean-ctx CLI):

| Variable | Purpose |
|----------|---------|
| `LEAN_CTX_PROXY_URL` | Override discovered base URL |
| `LEAN_CTX_PROXY_PORT` | Override port (default: UID-derived from 4444, or `proxy_port` in config) |
| `LEAN_CTX_PROXY_TOKEN` | Bearer token |
| `LEAN_CTX_DATA_DIR` | Data directory for `session_token` and `config.toml` |

Token resolution order: explicit `proxy_token` → `LEAN_CTX_PROXY_TOKEN` → `<data_dir>/session_token`.

Port resolution order: `LEAN_CTX_PROXY_PORT` → `proxy_port` in config.toml → UID-based port.

The wrap/session API does **not** use automatic port discovery unless you set `proxy_url` explicitly (including via `LEAN_CTX_PROXY_URL`).

## Receipt and Verification

### `verify()`

Returns `True` only when all checks pass:

1. `integrity_status == "sealed"` and canonical JSON is present
2. Local SHA-256 digest matches `canonical_hash` (`sha256:<hex>`)
3. If `signature` and a signer public key are present → Ed25519 verification (requires `pip install "lean-ctx-python[verify]"`)
4. Otherwise, if a verifier client is available → `GET /v1/receipts/{receipt_id}/verify` must return `verified: true`
5. If no signature and no verifier client → digest match alone is sufficient (local/mock environments)

Any failure or uncertainty returns `False`.

### Savings fields

Access via `receipt.savings` (`SavingsInfo`). All values are issued by the Runtime; the SDK does not derive costs from token headers.

| Field | Description |
|-------|-------------|
| `original_tokens` | Tokens before compression |
| `delivered_tokens` | Tokens after compression |
| `saved_tokens` | Tokens saved |
| `saved_pct` | Percentage saved (`None` if unknown) |
| `methodology` | How savings were measured (e.g. `compression_observation`, `baseline_treatment`) |
| `provider_input_tokens` | Provider-reported input tokens |
| `provider_output_tokens` | Provider-reported output tokens |
| `provider_cached_tokens` | Provider-reported cached tokens |
| `reasoning_tokens` | Reasoning tokens (when reported) |
| `baseline_cost_micros` | Baseline cost in micros |
| `treatment_cost_micros` | Treatment cost in micros |
| `avoided_cost_micros` | Avoided cost in micros |
| `baseline_ref` | Baseline reference identifier |
| `quality_status` | Quality assessment status |

When the proxy is unavailable and `fail_open=True`, receipts are explicitly **unsealed** with `methodology="unavailable"` and `verify()` returns `False`.

### Other receipt fields

`receipt_id`, `task_id`, `session_id`, `run_id`, `trace_id`, `agent_id`, `coverage`, `outcome`, `degradations`, `kits`, `profile`.

## Legacy compression API

For drop-in message compression without the wrap/session lifecycle:

```python
from lean_ctx import compress

messages = [{"role": "user", "content": long_text}]
compressed = compress(messages, model="gpt-4o")
```

`compress()` uses `ProxyClient` with automatic proxy discovery. Pass `base_url`, `token`, or `timeout` to override.

## Framework Integrations

### LangChain

Requires `pip install "lean-ctx-python[langchain]"`.

```python
from lean_ctx import compress_messages, LeanCtxRetriever

compressed = compress_messages(messages, model="gpt-4o")
retriever = LeanCtxRetriever(project_root="/path/to/repo", top_k=10)
docs = retriever.invoke("authentication flow")
```

`compress_messages` converts LangChain messages to the OpenAI wire shape, compresses via the proxy, and returns new messages with rewritten `content` only.

### LiteLLM

Requires `pip install "lean-ctx-python[litellm]"`.

```python
import litellm
from lean_ctx import LeanCtxLiteLLMHandler, compress_request_data

litellm.callbacks = [LeanCtxLiteLLMHandler(model="gpt-4o")]

# Or compress a request dict directly:
compress_request_data(request_data, model="gpt-4o")
```

The handler runs compression in a thread pool inside `async_pre_call_hook` so the synchronous `ProxyClient` does not block LiteLLM's event loop. On proxy failure, messages are left unchanged unless `raise_on_error=True`.

### LlamaIndex

Requires `pip install "lean-ctx-python[llamaindex]"`.

```python
from lean_ctx import LeanCtxNodeParser

parser = LeanCtxNodeParser(project_root="/path/to/repo", mode="map")
nodes = parser.get_nodes_from_documents(documents)
```

Compresses file-backed documents using the lean-ctx CLI `read` command with the chosen mode.

### OpenAI Agents SDK

Requires Python 3.10+ and `pip install "lean-ctx-python[openai-agents]"`.

```python
from agents import Agent, Runner
from lean_ctx import LeanCTX

agent = Agent(name="Assistant", instructions="Be concise and helpful.")
task = "Summarize the deployment plan."
result = Runner.run_sync(LeanCTX().wrap(agent), task)
print(result.final_output)
```

The `wrap()` path is available for the OpenAI Agents SDK. Running a live model still requires a provider key; for OpenAI, set `OPENAI_API_KEY`.

## Error Handling

### Exception hierarchy

| Exception | When |
|-----------|------|
| `LeanCtxError` | Base class; malformed responses, adapter errors, receipt parse failures |
| `LeanCtxConnectionError` | Proxy unreachable or HTTP 5xx |
| `LeanCtxAuthError` | HTTP 401/403 — missing or invalid token |

Configuration validation raises `ValueError` (unknown config keys, invalid timeouts, unsupported `integration_depth`).

### `fail_open` behavior

When `fail_open=True` (default):

- Proxy session start fails → agent runs unbound, receipt is unsealed with degradation `proxy_session_unavailable`
- Receipt sealing fails → unsealed receipt with degradation `receipt_sealing_failed`
- RunOnly agent with no proxy observations → degradation `provider_transport_not_bound`

When `fail_open=False`:

- Connection and auth errors propagate immediately
- Wrapping a RunOnly agent raises `LeanCtxError` at construction time

Agent exceptions during `run()` are always re-raised after the session is aborted; the SDK does not swallow agent errors.

## Development

From the package directory:

```bash
cd packages/python-lean-ctx
pip install -e ".[test,all]"
pytest
```

Tests use a local mock proxy (`conftest.py`) and do not require a running daemon.

### Contributing

Contributions are welcome in the main [lean-ctx repository](https://github.com/yvgude/lean-ctx). Run the full test suite before submitting changes:

```bash
pytest
python -m pytest tests/ -v
```

## Learn more

- [lean-ctx repository](https://github.com/yvgude/lean-ctx)
- [Installation guide](https://lean-ctx-python.com/docs/install)

## License

MIT
