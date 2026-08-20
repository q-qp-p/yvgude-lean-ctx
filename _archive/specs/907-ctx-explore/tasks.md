# Tasks: ctx_explore — FastContext-style iterative exploration  (refs #907)

> Atomic, individually testable. Cite the spec in commits:
> `<type>(<scope>): … refs specs/907-ctx-explore`.

- [ ] **T1 — core loop (#909)**
  - Files: `rust/src/tools/ctx_explore.rs`
  - Do: bounded multi-turn loop (default `max_turns=3`, env `LEAN_CTX_EXPLORE_*`);
    parallel side-effect-free retrieval channels; RRF fuse + cross-turn dedup;
    coverage-gap stop; `greedy_max_coverage` selection under token budget.
  - Verify: `cargo test -q ctx_explore` (loop-stop + determinism).
- [ ] **T2 — citation contract (#909)**
  - Files: `rust/src/tools/ctx_explore.rs`
  - Do: byte-stable `<final_answer>` block (`path:start-end  symbol (kind)`),
    sorted (score→path→line); `citation` option emits only the block.
  - Verify: citation-parse test; same input → identical bytes.
- [ ] **T3 — MCP + CLI wiring + artifacts (#910)**
  - Files: `rust/src/tools/registered/ctx_explore.rs`, `tools/mod.rs`,
    `tools/registered/mod.rs`, `server/registry.rs`, `cli/explore_cmd.rs`,
    `cli/mod.rs`, `cli/dispatch/mod.rs`, `cli/dispatch/help.rs`,
    `server/mod.rs` (count 77→78), generated manifest + reference docs,
    `rust/README.md`, `rust/src/templates/SKILL.md`.
  - Do: adapter mirrors `registered/ctx_compose.rs` (share `bm25_cache`,
    `block_in_place`, `ToolOutput`); register; route CLI; regen artifacts.
  - Verify: `cargo test -q entrypoints_wired mcp_manifest_up_to_date reference_docs_drift`.
- [ ] **T4 — determinism + tests (#911)**
  - Files: `rust/src/tools/ctx_explore.rs` (tests mod or sibling), `entrypoints_wired.rs`.
  - Do: `explore_output_is_byte_stable_across_calls` (warm + `isolated_data_dir`,
    savings footer off); add `explore` to the entrypoints probe list.
  - Verify: `scripts/preflight.sh full`; `cargo clippy -- -W clippy::all` zero warnings.
- [ ] **T5 — eval harness arm + tokens (#912)**
  - Files: `rust/src/core/eval_harness.rs`, `rust/eval/search-suite.ndjson`.
  - Do: `SearchArm::Explore` (extract paths from `<final_answer>`); per-arm
    `count_tokens` accounting; add explore queries to the suite.
  - Verify: `lean-ctx benchmark eval-ab --suite rust/eval/search-suite.ndjson --json`.
- [ ] **T6 — Phase 2 design (#913)**
  - Files: design note (issue #913 / spec appendix).
  - Do: `explorer-brain` addon manifest sketch (`[mcp]` stdio, `[capabilities]`),
    callback path, benchmark-before-recommend, FastContext license check.
  - Verify: reviewed; no core change.
- [ ] **T7 — docs (#914)**
  - Files: `CONTRIBUTING.md`, website tool docs.
  - Do: document `ctx_explore` (purpose, citation contract, vs `compose`).
  - Verify: doc/drift gates green.

## Done gate
- [ ] All EARS criteria covered by a task.
- [ ] `cargo fmt --check && cargo clippy --all-features -- -D warnings && cargo test --all-features`
- [ ] `scripts/preflight.sh full` green.
- [ ] GitLab #907–#914 statuses updated; spec referenced in commits.
