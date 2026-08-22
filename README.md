# LeanCTX

> **The Context SDK for AI Agents. Give every agent a context system.**

LeanCTX is a local context layer for existing AI agents. It selects, shapes,
reuses, and recovers the project context an agent needs inside the agent loop
you own. Your agent, model, tools, and task logic remain yours.

Your framework runs the agent. LeanCTX manages the context path routed through
it.

LeanCTX supports three ways to integrate:

- **Attach — Available** — add LeanCTX around a supported coding agent through
  CLI, MCP, or a local proxy.
- **Wrap — Preview** — use the declared Python SDK v1 reference-wrapper path
  with an explicit lifecycle and evidence boundary.
- **Embed — Preview** — integrate the Context Engine inside a custom host; the
  host keeps ownership of its full agent loop.

It is not an agent platform, a generic agent builder, a hosted execution
service, or a marketplace.

## What is available now

- Local Runtime, CLI, MCP, and local proxy paths.
- Context selection, structural views, compression, reuse, and recovery.
- Supported local setup paths for Codex, Claude Code, and Cursor.
- Local Receipt/evidence primitives and offline verification.

The **Python SDK v1** and its declared OpenAI Agents reference-wrapper are
**Preview**. Performance Profiles, first-class Context Kits, the Performance
Benchmark product flow, AutoTune, public rankings, and organization-scale
operation are **Research**. An implementation directory is not a product claim.
Local agent-presence, handoff, and related coordination substrate are also
**Research**; LeanCTX does not currently provide a public multi-agent
orchestration contract.

## The context lifecycle

```text
Select → Shape → Reuse → Recover
```

Evidence is a separate proof discipline: a gain is valid only with a comparable
baseline and treatment, a declared quality threshold, and visible methodology.
A lower token count or calculated cost is not a successful outcome on its own.

## Install

```bash
# Pick one installation method.
curl -fsSL https://leanctx.com/install.sh | sh
brew tap yvgude/lean-ctx && brew install lean-ctx
npm install -g lean-ctx-bin
cargo install lean-ctx

# Connect one supported agent, then verify the local installation.
lean-ctx wrap codex
lean-ctx doctor
```

Use `lean-ctx unwrap codex` to remove that integration, or `lean-ctx uninstall
--dry-run` to review a full removal before it changes anything.

## The context path

An agent does not always need the same representation of a project. LeanCTX
provides local tools to inspect structure, public interfaces, relevant excerpts,
exact lines, diffs, and full source. It also compresses eligible shell output,
keeps recoverable references to source, and exposes local context state for the
current task.

```text
agent → LeanCTX context tools / shell hook → project and local tools
agent → optional local proxy                 → model provider
```

The proxy only records and transforms traffic it can observe. Its data must not
be used to infer hidden prompts, retries, task quality, provider bills, or
accepted business savings.

## Use LeanCTX in your own agent

The current programmatic path is **Python SDK v1 (Preview)** through its
declared **OpenAI Agents reference-wrapper**. It wraps that one declared agent
lifecycle around the local Runtime; it does not choose a model, replace task
logic, or turn an unobserved run into verified evidence.

```bash
pip install lean-ctx-python
```

```python
from agents import Agent
from lean_ctx import LeanCTX

ctx = LeanCTX()
openai_agent = Agent(name="reviewer")
run = ctx.wrap(openai_agent).run("Review the payments module")
print(run.output)
print(run.receipt.verify())  # True only when Runtime evidence is sealed
```

See the [Python SDK README](packages/python-lean-ctx/README.md) for its declared
adapter scope, compatibility, degradation behaviour, and evidence boundary.

## Proof, not a percentage badge

Local observability can show context movement and compression deltas. A public
performance or savings claim needs a matched workload, baseline, treatment,
quality gate, methodology, and inspectable evidence. Until that path is complete,
token changes remain diagnostic signals rather than outcome claims.

## Supported setup paths

Codex, Claude Code, and Cursor are the current first-class local setup paths.
Other clients must expose their actual capability and degradation state; MCP
compatibility alone is not a support or evidence guarantee.

## Documentation

- [Developer documentation](docs/README.md)
- [Context SDK product architecture](docs/internal/vision/PRODUCT-ARCHITECTURE.md)
- [Current status and claim discipline](docs/internal/README.md)
- [Python SDK v1 Preview](packages/python-lean-ctx/README.md)
- [Security](SECURITY.md)
- [Changelog](CHANGELOG.md)

## Privacy and safety

LeanCTX is local-first. Telemetry is opt-in. Review the current configuration
and path boundaries before enabling any integration, and use `lean-ctx doctor`
to inspect local setup state.

## Uninstall

```bash
lean-ctx uninstall --dry-run
lean-ctx uninstall
```

If you installed through a package manager, use its uninstall command for the
binary after LeanCTX has removed its own local integration files.

## License

Apache-2.0. See [LICENSE](LICENSE).
