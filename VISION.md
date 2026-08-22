# LeanCTX Vision

> **Canonical sources:**
> [docs/internal/README.md](docs/internal/README.md) and
> [docs/internal/vision/PRODUCT-ARCHITECTURE.md](docs/internal/vision/PRODUCT-ARCHITECTURE.md).
> Those documents govern this summary. They win if wording or status differs.

## The product

**LeanCTX is the Context SDK for AI Agents.** It sits inside or alongside an
existing agent loop and controls how context is selected, shaped, reused,
recovered, and measured before inference.

LeanCTX does not replace the customer's agent, task logic, model choice,
tools, or retry policy. Thinkery is the company and commercial operator; it is
not a competing developer product.

The context lifecycle is:

```text
Select → Shape → Reuse → Recover
```

Context shapes performance. Evidence is separate: a valid gain compares the
same workload against a known baseline and treatment, with a declared quality
threshold and visible methodology. A cheaper failed task is not a win.

## Integration

| Depth | What it means | Status |
| --- | --- | --- |
| **Attach** | Add LeanCTX around an existing coding agent through CLI setup, MCP, or a proxy/sidecar. | **Available** locally; common v1 identity and Receipt semantics are **Preview**. |
| **Wrap** | Use a declared SDK/client adapter around a supported agent or client. | **Preview**. |
| **Embed** | Integrate LeanCTX natively in a custom agent or application. | **Preview**. |

Deeper integration increases observability and control; it never authorizes a
claim that the evidence cannot support.

## Status discipline

| Status | Meaning | Current scope |
| --- | --- | --- |
| **Available** | A local OSS capability has a real user path. | Runtime; CLI, MCP, proxy and local Attach paths; context selection, structural views, compression, reuse and recovery; local Receipt/evidence and offline-verification primitives. |
| **Preview** | A narrow contract is converging and must keep explicit compatibility and evidence limits. | Python SDK v1/reference-wrapper scope; Wrap and Embed contracts; common session and Receipt convergence; explicit capability and degradation matrices. |
| **Research** | A direction or private-commercial intent, not a public product promise. | Performance Benchmark; Performance Profiles; first-class Context Kits; canonical evidence bundle; AutoTune; organization control plane and LeanCTX Cloud; managed operation; external-capability composition; public benchmark/index; marketplace and agent-building. |

An implementation directory, a command, or an internal type is not by itself a
public API or shipping claim.

## Guardrails

- Local-first and model-agnostic: LeanCTX controls context, not the customer's
  agent or default model routing.
- Inspectable and recoverable: an optimized representation must preserve a
  path back to exact source when the task needs it.
- Evidence-led: savings require a comparable baseline, quality gate, cost
  basis, methodology, and verifiable evidence.
- No premature platform: hosted control planes, managed execution, marketplace
  surfaces, and autonomous tuning remain outside the current public OSS
  product.

Use the internal sources above for the authoritative feature map, product
boundary, vocabulary, and release status.
