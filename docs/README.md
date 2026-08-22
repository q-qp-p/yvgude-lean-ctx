# Repository docs

This folder contains **developer-facing** docs for the `lean-ctx` repository.

End-user documentation lives at **https://leanctx.com/docs/getting-started**.

## Start here

- Project overview: [`README.md`](../README.md)
- Contributing: [`CONTRIBUTING.md`](../CONTRIBUTING.md)
- Security: [`SECURITY.md`](../SECURITY.md)
- Architecture: [`ARCHITECTURE.md`](../ARCHITECTURE.md)
- Benchmarks: [`BENCHMARKS.md`](../BENCHMARKS.md)

## Codebase entry points

- Core binary + MCP server: [`rust/`](../rust/)
- Cookbook (real examples + `lean-ctx-client`): [`cookbook/`](../cookbook/)
- Editor integrations: [`packages/`](../packages/)

## Contracts & SDKs

Current product status is governed by [`docs/internal/README.md`](internal/README.md).
LeanCTX is a Context Performance SDK for existing agents: the local Runtime is
available; Python SDK v1 and its declared reference-wrapper scope are **Preview**.
Research and historical documents are not installation or availability claims.

- OCLA Wire Contract: [`docs/contracts/ocla-wire-v1.schema.json`](contracts/ocla-wire-v1.schema.json)
- Contract Pack (80+ documents): [`docs/contracts/`](contracts/)
- Contract Portal: [`docs/contracts/README.md`](contracts/README.md)
- Certification Levels: [`docs/contracts/certification-levels-v1.md`](contracts/certification-levels-v1.md)
- SDK status: [`docs/reference/sdk-conformance-matrix.md`](reference/sdk-conformance-matrix.md)
- Python SDK v1 (Preview): [`packages/python-lean-ctx/`](../packages/python-lean-ctx/)
- Rust client substrate: [`clients/rust/lean-ctx-client/`](../clients/rust/lean-ctx-client/)

## Reference & journeys

- Full function-by-function reference (organized as user journeys): [`reference/README.md`](reference/README.md)
- **User journeys (website narrative)** — the governed, scalable context runtime wave (MCP Gateway, Context Firewall, Sensitivity Floor, Reproducible Scorecard): [`user-journeys.md`](user-journeys.md)
- Always-current, generated appendices: [MCP tools](reference/generated/mcp-tools.md) · [config keys](reference/generated/config-keys.md)

## Guides

- Monorepo usage: [`guides/monorepo.md`](guides/monorepo.md)
- Publishing context packages to ctxpkg.com (sign, publish, install,
  lockfile): [`guides/publishing-packages.md`](guides/publishing-packages.md)

## Compliance

- Context Governance Benchmark (CGB) self-assessment — honest grading incl.
  declared gaps: [`compliance/cgb-self-assessment.md`](compliance/cgb-self-assessment.md)

## Design notes / tickets

- Cache correctness + heatmap plan: [`premium-cache-heatmap.md`](premium-cache-heatmap.md)

## Archive

- Implementation Protocol (v3.4.7 era): [`docs/archive/`](archive/)
