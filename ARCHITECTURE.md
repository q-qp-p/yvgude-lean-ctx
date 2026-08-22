# LeanCTX Implementation Inventory

> **Non-product document.** This path is an implementation-orientation stub,
> not an architecture promise, roadmap, feature list, or external API
> specification.

The canonical product architecture is
[docs/internal/vision/PRODUCT-ARCHITECTURE.md](docs/internal/vision/PRODUCT-ARCHITECTURE.md).
[docs/internal/README.md](docs/internal/README.md) governs authority,
terminology, scope, and status labels. Those two internal documents override
this file and all source-tree inferences.

## How to read the codebase

The repository contains runtime, CLI, MCP, proxy, hook, SDK, context-planning,
profile, Kit, receipt, benchmark, and capability-related implementation
substrate. A directory, module, test fixture, command, type, or diagram is
technical inventory only. It does not establish a public API, compatibility
commitment, availability, or commercial offering.

Use the following status map before describing an implementation:

| Area found in the repository | Product status | Claim boundary |
| --- | --- | --- |
| Local Runtime, CLI/MCP/proxy/Attach paths, context selection, structural views, compression, reuse, recovery, local Receipt and offline-verification primitives | **Available** | Useful local substrate with documented integration limits. |
| Session and plan contracts, Wrap/Embed adapters, Python SDK reference-wrapper scope, and common session/Receipt convergence | **Preview** | Converging contracts; implementation presence is not a stable general API. |
| Performance Profiles, first-class Context Kits, Performance Benchmark, AutoTune, hosted/control-plane work, marketplace or managed execution, public benchmark/index, and external-capability composition | **Research** | Direction or private-commercial work, not a public LeanCTX promise. |

The code's product boundary is deliberately narrow: LeanCTX is the Context SDK
for existing agents. It controls context before inference and does not replace
the customer's agent, task logic, model choice, tools, or retry policy.

## Maintenance rule

Keep implementation diagrams, module maps, generated counts, and historical
design detail in technical documentation or Git history. Before adding a public
description here, first promote its status in the canonical internal sources;
never infer it from implementation presence.
