# lean-ctx-python — Preview

> **Status: Preview.** This package documents one declared reference path:
> Python v1 with the OpenAI Agents SDK. It is not a general agent SDK, an
> automatic adapter layer, or evidence of support for every framework. Current
> product scope and status are governed by
> [`docs/internal/README.md`](../../docs/internal/README.md).

LeanCTX is **The Context SDK for AI Agents**. The reference wrapper connects an
existing OpenAI Agents SDK agent to a local LeanCTX Runtime. It does not replace
the agent, model, provider, or task logic.

## Reference path: OpenAI Agents SDK

Install the Preview package in a Python 3.10+ environment with the OpenAI Agents
SDK, and configure a local LeanCTX Runtime first.

```bash
pip install "lean-ctx-python[openai-agents]"
```

```python
from agents import Agent, Runner
from lean_ctx import LeanCTX

agent = Agent(name="Assistant", instructions="Be concise and helpful.")
task = "Summarize the deployment plan."

result = Runner.run_sync(LeanCTX().wrap(agent), task)
print(result.final_output)
```

This is a **Preview reference wrapper**, not a claim that every agent shape or
provider transport is supported. A live OpenAI run also requires the relevant
provider credentials.

## Evidence boundary

The local Runtime can emit receipt and offline-verification primitives. A
receipt makes a declared artifact inspectable; it does not by itself prove task
quality, a cost result, or a performance result. Any gain claim needs a
comparable baseline and treatment, a declared quality threshold, and visible
methodology.

## Not public SDK surface

The repository may contain experimental or internal interfaces for generic
`ctx.wrap` behavior, automatic adapter selection, LangChain, LiteLLM,
LlamaIndex, `load_kit`, `ContextKit`, `TuningProfile`, sessions, or custom
agent embedding. They are **not supported public Python SDK integrations**.
Context Kits, Performance Profiles, broader Embed work, and the canonical
evidence bundle remain Research; do not build a production integration against
those interfaces from this README.

For the Available, Preview, and Research boundary, see the internal
[Product Architecture](../../docs/internal/vision/PRODUCT-ARCHITECTURE.md).

## License

MIT
