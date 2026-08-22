# LeanCTX Implementation Surface Catalog

> **Non-product inventory.** This document does not define the LeanCTX vision,
> roadmap, public API, release status, tool count, compatibility surface, or
> commercial offer.

The only product authority is
[docs/internal/README.md](docs/internal/README.md) together with
[docs/internal/vision/PRODUCT-ARCHITECTURE.md](docs/internal/vision/PRODUCT-ARCHITECTURE.md).
When source code, generated tool metadata, an old release note, or this file
appears to conflict with those sources, the canonical internal documents win.

## Inventory rule

MCP tools, CLI commands, configuration keys, hooks, SDK classes, resource
templates, internal types, modules, and generated metadata are implementation
surface. They may be hidden, capability-gated, experimental, deprecated,
unsupported for a client, or awaiting a public contract. Their presence does
not make them a current user-facing feature.

Exact runtime discovery belongs to the running release and its generated
metadata. Do not repeat mutable tool counts, benchmark figures, or broad
compatibility claims here.

## Status-aware orientation

| Implementation area | Product status | Safe description |
| --- | --- | --- |
| Local Runtime; CLI, MCP, proxy and Attach paths; context selection, structural views, compression, reuse, recovery; Receipt and offline-verification primitives | **Available** | Local LeanCTX capability with explicit observability and integration limits. |
| Python SDK/reference-wrapper scope; Wrap and Embed contracts; context plans; common session/Receipt convergence | **Preview** | Converging, bounded contract — never a general availability claim. |
| Performance Profiles, first-class Context Kits, Performance Benchmark, AutoTune, control planes, hosted operation, marketplace, managed execution, public benchmark/index, external-capability composition, and agent-building | **Research** | Design direction or private-commercial scope, not current public LeanCTX product surface. |

LeanCTX remains the Context SDK for existing AI agents: it controls how context
is selected, shaped, reused, recovered, and measured before inference. It does
not become an agent platform because related implementation substrate exists.

## Maintenance rule

Before documenting a command or module as a product feature, verify its status
against the canonical internal sources and state the corresponding boundary.
Preserve superseded inventories in Git history rather than presenting them as
current truth.
