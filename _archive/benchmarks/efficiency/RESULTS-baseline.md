# lean-ctx Efficiency — Baseline + Phase 1 Results

Frozen reference for the efficiency epic. Reproduce with:

```bash
cd rust && cargo bench --bench efficiency        # 2000-file synthetic corpus
```

The harness measures the **walk path** (index forced off via
`LEAN_CTX_DISABLE_SEARCH_INDEX=1`) and the **resident index path** (warmed
synchronously) on the same corpus and queries, asserting recall parity
(identical `file:line` hits, Jaccard = 1.0) before reporting latency.

## `ctx_search` — synthetic-2000 (2000 files, 50 iters)

### Walk path (legacy baseline)

| query | p50 ms | p95 ms | p99 ms | resp tokens |
|---|---|---|---|---|
| common (`handler`) | 4.649 | 4.953 | 5.806 | 427 |
| rare (`flushPassiveEffectsRare`) | 25.841 | 27.944 | 28.506 | 108 |
| negative (`xyzzy_nonexistent`) | 26.153 | 27.762 | 28.743 | 14 |

### Resident index path (Phase 1)

| query | p50 ms | p95 ms | p99 ms | resp tokens |
|---|---|---|---|---|
| common (`handler`) | 0.265 | 0.386 | 0.483 | 427 |
| rare (`flushPassiveEffectsRare`) | 0.088 | 0.103 | 0.116 | 107 |
| negative (`xyzzy_nonexistent`) | 0.026 | 0.029 | 0.031 | 13 |

### Speedup (p50)

| query | walk | index | speedup |
|---|---|---|---|
| common | 4.649 ms | 0.265 ms | **17.5×** |
| rare | 25.841 ms | 0.088 ms | **293×** |
| negative | 26.153 ms | 0.026 ms | **~1000×** |

## Interpretation

- **Phase 1 acceptance met.** p50 (warm) drops well below the 5 ms target on
  every query; recall is byte-identical (parity assertion passes, `warm=true`).
- The rare/negative queries gain the most: trigram narrowing reads ~0 candidate
  files instead of all 2000. A negative query short-circuits to `O(1)` (a
  required trigram is absent from the index).
- Response token counts are unchanged — the win is pure latency, no behavioral
  change to what the agent receives. (The 1-token deltas on the no-match
  messages are the "N files searched" count: candidates vs. full corpus.)

## Notes on the token/agentic axis

Per-call response tokens are intentionally unchanged in Phase 1 (latency-only).
The total-task-token and tool-call reductions come from Phases 2-4
(`ctx_compose`, inline read bodies, symbol-map default-off) and are measured via
the agentic mini-eval protocol in `METHODOLOGY.md`.
