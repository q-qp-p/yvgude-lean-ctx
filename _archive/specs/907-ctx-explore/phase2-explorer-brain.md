# Phase 2 design — `explorer-brain` addon (refs #907, #913)

> Design note only. **No core change.** Phase 1 (`rust/src/tools/ctx_explore.rs`)
> ships the deterministic, model-free loop and stays the default. Phase 2 adds an
> **optional, opt-in** addon that delegates the *reasoning* turn to a specialized
> small model (the FastContext "4B explorer" idea), while lean-ctx keeps owning
> retrieval, grounding and the citation contract.

## 1. Why an addon (not core)

The Phase 1 loop is a pure function of (repo, query, options) — BM25 anchor →
static-graph BFS → AST symbols → submodular selection. That determinism is its
strength (#498: byte-stable, cache-friendly) but also its ceiling: it cannot
*reason* about which neighbour to follow when lexical + structural signals are
weak. A small, specialized exploration model can — at the cost of determinism and
a model dependency.

Putting the model behind the **addon boundary** keeps that cost optional and
isolated:

- core has zero new heavy deps and stays deterministic for everyone;
- the model runs as a sandboxed `stdio` MCP server (existing trust pipeline);
- users who want it opt in explicitly and consent to its capabilities.

## 2. Architecture

```
ctx_explore::handle (core, Phase 1)
  │  lexical anchor + candidate frontier (deterministic)
  ▼
delegate? ── no ──► Phase-1 selection (unchanged, default)
  │ yes (addon installed + LEAN_CTX_EXPLORE_BRAIN=1)
  ▼
gateway → explorer-brain (stdio MCP addon)
  │  input:  query + candidate spans (path:line + tiny snippets)
  │  model:  rank / pick next frontier  (NON-deterministic)
  │  callback (exec=["lean-ctx"]): ctx_read / ctx_search for grounding
  ▼
core re-grounds the addon's picks → SAME citation contract (<final_answer>)
```

Core stays the **trust root**: the addon never emits citations directly. It
returns *suggestions* (file/symbol ids from the candidate set); core validates
each against the live index and renders the byte-stable block. A hallucinated
path that isn't in the candidate set is dropped, not cited.

## 3. Addon manifest sketch (manifest v1)

```toml
[addon]
slug = "explorer-brain"
author = "..."
homepage = "https://github.com/.../explorer-brain"   # public, inspectable source
license = "Apache-2.0"                                 # see §6
description = "Specialized small-model code exploration brain for ctx_explore."
version = "0.1.0"

[mcp]
transport = "stdio"
command = "explorer-brain"
args = ["--serve"]
sha256 = "<pinned>"          # required for a verified listing (fail-closed)

[capabilities]
# Local-inference build: no egress. A hosted-API build would declare
# network = "full" + the provider host, and is disclosed at install.
network = "none"
filesystem = "read_only"     # reads repo files it is handed; writes nothing
exec = ["lean-ctx"]          # callback path: may spawn ONLY lean-ctx (§4)
env = []                     # no host secrets for the local build

[pricing]                    # optional; omit for free
# model = "usage"
# usage_price_per_1k_cents = 50
```

Capability rationale (coherence-checked by `core::addons::trust::assess`):
`exec=["lean-ctx"]` is the only privilege beyond reading; `network="none"` keeps
the local build air-gapped so it cannot exfiltrate the candidate context.

## 4. Callback path (grounding)

The brain is allowed to *ask for more context* instead of guessing, via the
declared `exec = ["lean-ctx"]` capability:

- `lean-ctx -c 'ctx_read <path> --range a-b'` to inspect a candidate span;
- `lean-ctx explore <refined-query> --citation` to recurse one bounded level.

Egress/write limits are **inherited by the child** (OS sandbox), so the callback
cannot escape the addon's profile. The protocol between core and brain is a
small, versioned JSON envelope:

```jsonc
// core → brain
{ "query": "...", "candidates": [{ "id": 0, "path": "src/a.rs", "lines": "10-40", "snippet": "..." }], "budget_tokens": 1200 }
// brain → core
{ "pick_ids": [0, 7], "expand_ids": [3], "stop": false }   // ids only; core re-grounds
```

## 5. benchmark-before-recommend (gate, reuses the eval harness)

lean-ctx must **never** recommend the addon on vibes. Phase 2 reuses the harness
extended in T5 (`SearchArm::Explore` + per-arm `count_tokens`). Add a third arm
that routes through the addon and decide with the existing `decide_verdict`
machinery:

- run `lean-ctx benchmark eval-ab --suite rust/eval/search-suite.ndjson --json`
  with the brain installed;
- **recommend only if** the brain arm beats the deterministic Phase-1 arm on
  recall@5 by a margin **and** stays within a token budget (recall-per-token must
  not regress) on the curated suite;
- otherwise keep Phase 1 — it is free, deterministic and already shipped.

This makes "is the model worth it?" an empirical, reproducible question, not a
marketing claim.

## 6. FastContext license check

If the recommended build derives from FastContext (arXiv 2606.14066) weights or
code, recommendation is gated on license compatibility:

- the manifest `license` is **required** (the registry validator already rejects
  installable entries without it);
- before lean-ctx surfaces an in-product recommendation, verify the declared
  license is OSI-compatible with redistribution/recommendation (e.g. Apache-2.0 /
  MIT). A research-only or non-commercial license → **list, do not recommend**;
- the check is a registry-review step (human + validator), recorded on the
  registry entry; no automatic bundling of weights.

## 7. Determinism boundary (explicit)

Phase 2 is model-driven and therefore **non-deterministic** — it lives *outside*
the #498 byte-stable contract by design. Guardrails:

- opt-in only (`LEAN_CTX_EXPLORE_BRAIN=1` + addon installed);
- output still flows through core's citation renderer, so the *envelope* and
  grounding invariants hold even though the *selection* is not byte-stable;
- with the addon absent or the flag unset, `ctx_explore` is bit-for-bit Phase 1.

## 8. Non-goals / open questions

- Not bundling a model in core or in the lean-ctx binary.
- Streaming partial citations (defer; Phase 1 returns a single block).
- Caching brain decisions for partial determinism (possible later; out of scope).
- Multi-model routing (pick brain by language/repo size) — future.
