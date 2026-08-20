# Spec: ctx_explore — FastContext-style iterative exploration  (refs #907)

> SDD spec-anchored: this file is the source of truth for **intent**.
> Code + tests enforce it. When requirements change, update the spec first.

## Problem / Why
Microsoft **FastContext** (arXiv 2606.14066) shows that delegating *exploration*
to a focused sub-loop — which returns compact `file:line` citations instead of raw
file dumps — cuts main-agent tokens by ~60% with a small end-to-end quality gain.
lean-ctx already has a stronger retrieval substrate (BM25 + hybrid + graph
spreading + SPLADE + AST) than FastContext's grep loop, but its closest tool,
[ctx_compose](../../rust/src/tools/ctx_compose.rs), is **single-shot** and emits
prose Markdown rather than a parseable citation block. The gap is an **iterative,
bounded, deterministic exploration loop with a stable citation contract**.

## Goal
A new `ctx_explore` MCP tool (plus `lean-ctx explore` CLI) that runs a bounded
multi-turn retrieval loop over lean-ctx's existing engines and returns a
byte-stable `<final_answer>` block of `path:start-end` citations.

## Acceptance Criteria (EARS)
- **#909** WHEN `ctx_explore` is called with a natural-language query, THE tool
  SHALL return a `<final_answer>` block whose lines are `path:start-end` citations.
- **#909** THE explore loop SHALL be bounded (`max_turns`, default 3) AND SHALL
  stop early once additional turns add no new covered files (coverage saturation).
- **#909** THE tool output SHALL be a deterministic function of (repo content,
  query, options): it SHALL use only side-effect-free retrieval paths
  (`search_hits`, static graph) and SHALL NOT write co-access/session state in its
  output path (#498).
- **#909** WHEN the `citation` option is true, THE tool SHALL emit only the
  `<final_answer>` block (no prose summary).
- **#910** THE MCP registry SHALL advertise `ctx_explore` AND THE CLI SHALL route
  `lean-ctx explore <query>`; neither SHALL fall through to help (#902 gate).
- **#910** THE registered tool-count SSOT SHALL be 78 AND the generated MCP
  manifest + reference docs SHALL be regenerated to include `ctx_explore`.
- **#911** THE test suite SHALL prove byte-stability (`assert_eq!(run(), run())`
  over a warm, isolated index) and citation-block parseability.
- **#912** THE eval harness SHALL expose an `Explore` arm and per-arm token
  accounting so `benchmark eval-ab` can compare recall/MRR **and** tokens.

## Out of Scope
- The FastContext model itself (a small specialist LLM): deferred to **Phase 2**
  as an external `explorer-brain` addon (#913) — non-deterministic, sandboxed.
- Changing `ctx_compose` behavior or any `docs/contracts/*-v1.md`.
- Adaptive/learned ranking signals (Hebbian co-access, session memory, heatmap)
  inside the byte-stable block — they stay out of Phase 1's deterministic core.

## Verification (deterministic first)
- `cargo test -q ctx_explore` (determinism, citation parse, loop stop)
- `cargo test -q entrypoints_wired` (explore wired in MCP + CLI)
- `cargo test -q mcp_manifest_up_to_date` · `cargo test -q reference_docs_drift`
- `scripts/preflight.sh full`
- `lean-ctx benchmark eval-ab --suite rust/eval/search-suite.ndjson --json`

## Links
- GitLab: #907 (epic) → #908 #909 #910 #911 #912 #913 #914
- Plan: ./plan.md · Tasks: ./tasks.md
- Related contract: output determinism (#498)
