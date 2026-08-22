# LeanCTX Runtime

> **Status: implementation guide.** LeanCTX is **The Context SDK for AI
> Agents**: a local context-performance layer for existing agents. It selects,
> shapes, reuses, recovers, and measures context before inference. This directory
> contains the Runtime that powers the available local CLI, MCP server, proxy,
> context primitives, and local evidence/offline-verification paths.
>
> It is not a generic agent builder, agent platform, hosted execution service,
> marketplace, control plane, or organization product. Python SDK v1 and its
> declared OpenAI Agents reference wrapper are **Preview**. Performance
> Benchmark, Profiles, first-class Context Kits, AutoTune, public rankings, and
> Cloud/managed/team/SSO surfaces are **Research** or unavailable. Product scope
> and status are governed by
> [`docs/internal/README.md`](../docs/internal/README.md).

## Build locally

```bash
cd rust
cargo build --release
```

This build writes only to the worktree. Do not stop an installed Runtime before
building or testing. For a local development install, use `lean-ctx dev-install`
after the checks below succeed.

## Validate a change

```bash
cd rust
cargo test --lib
cargo clippy --all-features -- -D warnings
cargo fmt --check
```

## Use the local Runtime

```bash
lean-ctx setup
lean-ctx doctor
lean-ctx wrap codex
```

The Runtime supports context selection, compression, reuse, recovery, and
local evidence. Measure a result against a comparable baseline and treatment,
declare a quality threshold, and keep the methodology visible. A cheaper failed
task is not a gain.

## Integration boundaries

- **Attach:** use the installed CLI, MCP server, or local proxy.
- **Wrap:** use the declared adapter path where available; discover capabilities
  and report typed limitations when a capability is unavailable.
- **Embed:** a custom application/agent integration remains a bounded Preview
  target, not a general-purpose agent-building surface.

See the repository [README](../README.md) for installation and current public
orientation. See [`docs/internal/vision/PRODUCT-ARCHITECTURE.md`](../docs/internal/vision/PRODUCT-ARCHITECTURE.md)
for the canonical status map.
