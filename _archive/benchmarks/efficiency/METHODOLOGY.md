# lean-ctx Efficiency Benchmark — Methodology

Reproducible harness for the **lean-ctx Efficiency Epic**. It proves every phase
on two axes that actually matter for an agent loop: **wall-time latency**
(p50/p95/p99) and **total response tokens** (`tiktoken`, via the same counter
the tools use), measured MCP-resident (the way the agent actually pays for it).

## Why these axes

Per-call byte savings can be misleading: aggressive compression that shaves a
few bytes per response often forces the agent into extra reads, raising **total
task tokens and tool-calls**. The optimization target is therefore **total task
tokens + wall-time**, never per-call bytes. The harness measures exactly that.

## Latency + tokens (runnable now)

Custom Rust harness (`rust/benches/efficiency.rs`, `harness = false`):

```bash
cd rust
cargo bench --bench efficiency            # 2000-file synthetic corpus
BENCH_FILES=6000 cargo bench --bench efficiency   # react-scale corpus
```

It builds a deterministic synthetic corpus (files across 20 dirs, a common
token, a rare camelCase token, and a guaranteed-absent negative query), warms
once, then runs `ITERS=50` and reports `p50/p95/p99` ms plus response tokens.

From Phase 1 on, the harness emits two blocks — **Walk path (legacy)** and
**Resident index** — on the same corpus and queries, so the speedup is visible
in a single run with no "before" git checkout.

### Corpora

- **self** — the lean-ctx Rust tree (`rust/`), real-world mixed file sizes.
- **synthetic-2000 / synthetic-6000** — deterministic, react-scale, CI-stable.

## Agentic mini-eval (protocol)

The latency/token harness cannot exercise an LLM loop, so the agentic axis is a
documented protocol run against a real model with the MCP server attached. Use
3-4 natural-language tasks per corpus and record **tool-calls, wall-time, total
tokens, quality (pass/fail rubric)**:

1. "Where is the search index built and how does ctx_search use it?"
2. "Find the function that flushes passive effects and show its body."
3. "Add a parameter to the BM25 cache TTL and list every call site."
4. "Trace how a provider result reaches the BM25 index."

Freeze the baseline numbers in `RESULTS-baseline.md`, then re-run after each
phase and diff. Acceptance for a phase is **fewer-or-equal tool-calls and
total tokens at equal-or-better quality** vs. the frozen baseline.

## Recall parity (Phase 1 gate)

The resident index must not change *which* lines `ctx_search` returns. The
harness asserts set-equality of `file:line` hits between the walk path and the
index path (Jaccard ≥ 0.95) on every query before reporting latency.
