# SDK status and conformance evidence

Current product status is governed by
[`docs/internal/README.md`](../internal/README.md) and the active
[SDK v1 specification](../internal/execution/SDK-V1-SPEC.md).

| Surface | Current status | Canonical location | Claim boundary |
| --- | --- | --- | --- |
| Local Runtime, CLI, MCP, proxy | Available | `rust/` | Local context-performance substrate; not a hosted agent platform. |
| Python SDK v1 | Preview | `packages/python-lean-ctx/` | `lean-ctx-python` 1.0.0 with primary import `lean_ctx`; its reference-wrapper and evidence scope are explicit. |
| Rust client | Substrate | `clients/rust/lean-ctx-client/` | A client implementation, not a separate promoted SDK product. |
| TypeScript and Go SDKs | Not current surfaces | `_archive/` | No current distribution, support, or conformance claim. |

Conformance evidence must be generated from a pinned canonical package and a
live Runtime. Do not infer support from archived code, schema fixtures, or a
manual multi-SDK table.
