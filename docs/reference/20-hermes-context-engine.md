# Preview — embedding LeanCTX into a custom agent

> **Reference integration only.** This document does not describe a general
> agent framework, a replacement agent loop, or a stable public Embed contract.

## Product boundary

LeanCTX sits inside or alongside an existing agent. In an Embed integration, the
host continues to own the agent loop, task logic, model choice, tools, retries,
and application behavior. LeanCTX contributes the context lifecycle:
selection, representation, reuse, recovery, and measurement before inference.

The current Engine and adapter code are implementation substrate. The planned
public LeanCTX/session facade, typed adapter semantics, and common receipt flow
are still **Preview**. The Python SDK v1 has only its declared OpenAI Agents
reference-wrapper scope; it is not a general framework adapter.

## Hermes material

The repository's Hermes-related files are an engineering reference for
evaluating one host integration. They must be read as a bounded experiment with
explicit observability and recovery limits, not as a supported product
integration or a claim that LeanCTX takes ownership of the host context window.

## Evaluation rule

Test an Embed path against a named workload and state exactly what the host
provides and what LeanCTX can observe. Any result needs the same quality gate,
baseline/treatment comparison, and evidence discipline as every other LeanCTX
integration.
