# Plan: ctx_explore — FastContext-style iterative exploration  (refs #907)

> Implementation plan for `./spec.md`. Review before tasks.

**Goal:** A bounded, deterministic multi-turn exploration tool that returns a
byte-stable `<final_answer>` citation block over lean-ctx's retrieval engines.
**Architecture:** A pure handler (`rust/src/tools/ctx_explore.rs`) drives N bounded
turns; each turn fans out to side-effect-free retrieval channels
(`ctx_semantic_search::search_hits`, `graph_provider::find_symbols`, call graph,
exact `ctx_search`), fuses with `reciprocal_rank_fusion`, dedups across turns,
stops on coverage saturation, then selects citations with `greedy_max_coverage`
under a token budget. A thin `McpTool` adapter mirrors `registered/ctx_compose.rs`.
**Tech Stack:** Rust, existing core crates; no new dependencies.

## Global Constraints
- Do **not** modify `ctx_compose` behavior or `docs/contracts/*-v1.md`.
- Deterministic core only: `search_hits` (not the session-writing `handle`),
  static graph **without** `cooccurrence::record_access`. No timestamps / counters
  / random / session writes in the output body (#498).
- No mock data, no placeholders, no stubs.
- Adaptive/learned signals excluded from the byte-stable block (Phase 2 territory).

## File Structure
| File | Responsibility | New/Modify |
|------|----------------|------------|
| `rust/src/tools/ctx_explore.rs` | core loop + citation rendering (#909) | new |
| `rust/src/tools/registered/ctx_explore.rs` | `CtxExploreTool: McpTool` (#910) | new |
| `rust/src/tools/mod.rs`, `rust/src/tools/registered/mod.rs` | module wiring | modify |
| `rust/src/server/registry.rs` | `registry.register(CtxExploreTool)` (#910) | modify |
| `rust/src/cli/explore_cmd.rs` | `lean-ctx explore` (#910) | new |
| `rust/src/cli/mod.rs`, `rust/src/cli/dispatch/mod.rs`, `rust/src/cli/dispatch/help.rs` | CLI route + help | modify |
| `rust/src/server/mod.rs` | tool-count SSOT 77→78 (#910) | modify |
| MCP manifest + reference docs (generated) | regen to include explore (#910) | modify |
| `rust/README.md`, `rust/src/templates/SKILL.md` | "77"→"78" | modify |
| `rust/tests/entrypoints_wired.rs` | add `explore` probe (#911) | modify |
| `rust/src/core/eval_harness.rs`, `rust/eval/search-suite.ndjson` | Explore arm + tokens (#912) | modify |

## Impact (run impact analysis first)
> Run `ctx_impact` / `lean-ctx graph impact <file>` before editing dispatch/registry.
- Affected gates (the verification set): `entrypoints_wired`,
  `mcp_manifest_up_to_date`, `reference_docs_drift`, `test_registry_tool_count_ssot`.
- Affected modules to inspect: `rust/src/server/registry.rs`,
  `rust/src/server/dispatch/**`, `rust/src/cli/dispatch/**`, `rust/src/tool_defs/**`.
- Reused (read-only) building blocks: `ctx_semantic_search::search_hits`,
  `hybrid_search::reciprocal_rank_fusion`, `graph_provider::{find_symbols,
  related_files_scored}`, `call_graph`, `task_relevance::parse_task_hints`,
  `context_packing::greedy_max_coverage`, `tokens::count_tokens`,
  `ctx_symbol::best_symbol_snippet`.

## Self-Review (fill before implementing)
- Spec coverage: #909↔T1/T2/T4, #910↔T3, #911↔T4, #912↔T5, #913↔T6, #914↔T7.
- Placeholder scan: real retrieval APIs only; no TODO/mock/fallback.
- Determinism: byte-stable test asserts `run() == run()` on warm isolated index;
  citation block sorted (score→path→line); no side effects in output path.
- Cleanup: no scratch files; temp benchmark artifacts removed.
