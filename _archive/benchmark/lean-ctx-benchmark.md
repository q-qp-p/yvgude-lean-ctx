# lean-ctx — One Binary, Same Savings as 6 Tools Combined

**Response to: [Claude Code Token Savings Stack — 6 layers, zero overlap, ~60% context reduction](https://gist.github.com/doobidoo/e5500be6b59e47cadc39e0b7c5cd9871)**

What if one binary covers all 6 layers?

That's lean-ctx — a single Rust binary / MCP server that handles CLI compression, file read optimization, response compression, targeted context, and session knowledge. No 6 repos, no 3 programming languages, no 15-minute setup dance.

```
User Prompt
  → [lean-ctx ctx_knowledge]     Cross-session memory → skip re-discovery
  → [lean-ctx ctx_read modes]    Targeted context (map/signatures/aggressive) → skip full reads
    → LLM thinks → [CRP mode]   Compact responses → no filler tokens
      → Tool calls → [lean-ctx MCP]  51 tools, compact schemas
        → Bash/CLI → [lean-ctx -c]   56 pattern modules → structured compression
          → File reads → [lean-ctx cache]  Re-reads cost ~13 tokens
```

One install. One binary. Same result.

---

## Benchmark Setup

- **System:** macOS, lean-ctx 3.6.6
- **Project:** lean-ctx itself (Rust + TypeScript + Python, ~50K LOC)
- **Tokenizer:** tiktoken cl100k_base (GPT-4/Claude tokenizer, exact counts — not char/4 approximation)
- **Measurement:** Built-in `lean-ctx benchmark` command using Rust tiktoken bindings
- **Reproducible:** `lean-ctx benchmark run . --json` generates raw data

---

## Results: Layer-by-Layer Comparison

### 1. CLI Output Filtering (RTK equivalent)

lean-ctx intercepts shell commands via `lean-ctx -c` and applies 56 domain-specific compression modules (git, cargo, npm, docker, terraform, kubectl, etc.).

**Measured on 1,794 real commands (production usage):**

| Metric | lean-ctx | RTK |
|--------|----------|-----|
| Commands measured | 1,794 | 282 |
| Total input tokens | 91,344,503 | ~195K (estimated from 117K saved at 60%) |
| Tokens saved | 54,733,233 | 117,100 |
| **Savings rate** | **59.9%** | **60.2%** |
| Avoided cost (USD) | $136.83 | — |

**Per-command examples:**

| Command | Raw | Compressed | Savings |
|---------|-----|-----------|---------|
| `git log --stat -10` | 8,693 chars | 636 chars | 92.7% |
| `git diff HEAD~5 --stat` | 3,077 chars | 179 chars | 94.2% |
| `git log --oneline -50` | 3,431 chars | 1,221 chars | 64.4% |
| `git status` | 2,350 chars | 1,585 chars | 32.6% |

lean-ctx doesn't blindly filter — it pattern-matches structured output. Git stat blocks become one-liners, test results become summaries, verbose logs become actionable diffs.

---

### 2. File Read Compression (context-mode equivalent)

Instead of dumping raw files into context, lean-ctx auto-selects the optimal read mode per file.

**50 files measured across 9 languages (tiktoken exact counts):**

| Read Mode | Avg Savings | Quality Preserved | Use Case |
|-----------|-------------|-------------------|----------|
| `map` | 97.4% | 81% | Dependencies + API surface |
| `signatures` | 96.6% | 90% | Function/class signatures only |
| `cache_hit` | 99.8% | — | Re-reads from session cache |
| `aggressive` | 4.1% | 100% | Full content, comments stripped |
| `entropy` | 0.5% | 100% | Full content, high-entropy only |

**Per-language best savings:**

| Language | Files | Raw Tokens | Best Mode | Savings |
|----------|-------|-----------|-----------|---------|
| .rs | 10 | 144,295 | map | 96.5% |
| .md | 10 | 80,376 | aggressive | 5.6% |
| .js | 10 | 71,352 | map | 99.1% |
| .json | 5 | 67,430 | aggressive | 0.5% |
| .py | 9 | 26,688 | signatures | 94.5% |
| .css | 1 | 18,049 | aggressive | 2.4% |
| .ts | 4 | 13,974 | map | 95.6% |
| .html | 1 | 8,656 | aggressive | 2.4% |

> **Note on non-code files:** Markdown, JSON, CSS, HTML are data/markup files without code structures (functions, classes, types). lean-ctx's structural modes (`map`, `signatures`) extract code skeletons and are only applicable to programming languages. For data/markup files, only `aggressive` mode (whitespace/comment stripping) is reported. The high savings for code files (Rust 96.5%, Python 94.5%, JS 99.1%) come from extracting only the structural skeleton that an LLM needs for context.

**vs. context-mode:** context-mode sandboxes output into SQLite and returns BM25 snippets (~98% claimed). lean-ctx achieves 96-99% on code files through intelligent mode selection — no database, no indexing delay, deterministic results.

---

### 3. Tool Definition Size (MCPlex equivalent)

| Setup | Tools Exposed | Token Cost |
|-------|---------------|-----------|
| 6-tool stack (raw) | 37 tools | ~8,762 tokens |
| MCPlex gateway | 3 meta-tools | ~273 tokens |
| **lean-ctx** | **51 tools** | **~3,200 tokens** |

lean-ctx exposes all tools directly with compact JSON schemas. No gateway needed, no `find_tools()` indirection, no semantic routing overhead. The LLM sees all capabilities immediately.

**Trade-off:** MCPlex wins on raw token count (273 vs 3,200) by hiding tools. But lean-ctx tools are directly callable — no discovery round-trip needed, which saves 1-2 tool calls per interaction.

---

### 4. Response Compression (Caveman equivalent)

CRP (Compact Response Protocol) compresses tool responses in-flight:

| Mode | Tokens (30-min session) | Cost |
|------|------------------------|------|
| Raw (no lean-ctx) | 605,400 | $1.51 |
| lean-ctx | 84,400 | $0.21 |
| lean-ctx + CRP | 79,900 | $0.20 |

**CRP savings over lean-ctx alone: additional ~5.4% compression** through abbreviations, delta-only diffs, and structured `+/-/~` notation.

**vs. Caveman (20-40% on output):** CRP operates at the tool-output level, not the LLM response level. They're complementary — you could use both. But lean-ctx's modes already deliver the bigger wins upstream.

---

### 5. Targeted Context (MCP-Context-Provider equivalent)

Instead of a separate server providing context rules, lean-ctx's 10 read modes ARE the targeted context:

```
Developer asks: "How does auth work?"

Without lean-ctx:
  → Read auth.rs (full)         = 2,500 tokens
  → Read middleware.rs (full)   = 1,800 tokens
  → Read config.rs (full)       = 900 tokens
  Total: 5,200 tokens

With lean-ctx (auto-mode):
  → ctx_read auth.rs mode=map       = 65 tokens (deps + API)
  → ctx_read middleware.rs mode=map  = 42 tokens
  → ctx_read config.rs mode=map     = 18 tokens
  Total: 125 tokens (97.6% less)
```

No separate service. No rule configuration. The compression IS the context targeting.

---

### 6. Session Knowledge (MCP-Memory-Service equivalent)

lean-ctx provides cross-session persistence without a vector database:

| Feature | lean-ctx | MCP-Memory-Service |
|---------|----------|-------------------|
| Cross-session memory | `ctx_knowledge remember/recall` | `memory_store/search` |
| Session state | `ctx_session` (auto-compaction) | — |
| Re-read cost | ~13 tokens (cached) | N/A |
| Warm start | `ctx_preload` | Embedding search |
| Storage | Local files (instant) | SQLite + Cloudflare Vectorize |
| Setup | Zero config | API tokens, cloud setup |

**Measured re-read savings:**
- First read of 10 source files: ~15,000 tokens
- Re-read (session cache): ~130 tokens (10 × ~13 tok)
- Knowledge recall: ~200-500 tokens

**Effective savings: 95-99% on repeated access.**

---

## Session Simulation: Combined Savings

**30-minute coding session (50 files, multiple reads, shell commands):**

| Setup | Tokens | Cost | Savings |
|-------|--------|------|---------|
| Raw (no compression) | 605,400 | $1.51 | — |
| lean-ctx (all modes) | 84,400 | $0.21 | **86.1%** |
| lean-ctx + CRP | 79,900 | $0.20 | **86.8%** |

**The 6-tool stack claims ~58.5% savings. lean-ctx measured 86.1-86.8%.**

---

## Why the Difference?

The 6-tool stack operates at different layers that don't compose perfectly. lean-ctx is architecturally integrated:

1. **No inter-tool overhead** — One process, one cache, one tokenizer
2. **Mode selection is aware of context** — The cache knows what was already sent
3. **Re-reads are essentially free** — Session-aware caching eliminates redundant I/O
4. **Shell + file reads compound** — The same session state optimizes both

---

## Methodology & Transparency

- **Token counting:** Rust bindings to tiktoken (cl100k_base) — exact token counts, not char/4 approximation
- **"Best mode" selection:** Only modes that produce meaningful output qualify. A mode returning 0 tokens (e.g., `map` on JSON) is excluded — that's data loss, not compression
- **Quality score:** Semantic preservation measured via key-symbol retention (exported names, types, function signatures)
- **Reproducibility:** Run `lean-ctx benchmark run /your/project --json` on any codebase
- **Not cherry-picked:** Benchmark runs on ALL files matching configured extensions, not hand-selected examples

---

## Install

```bash
# One command. 30 seconds. Done.
cargo install lean-ctx

# Or from source:
git clone https://github.com/yvgude/lean-ctx
cd lean-ctx/rust && cargo build --release
```

vs. the 6-tool stack:
```bash
# 6 repos, 3 languages, 15 minutes, bridge configs...
cargo install rtk
git clone mcplex && cargo build
git clone MCP-Context-Provider && npm install && npm run build
git clone mcp-memory-service && uv sync
# + Claude Code plugin installs
# + MCPlex upstream configuration
# + bridge.mjs for macOS...
```

---

## Reproduce This Benchmark

```bash
# After installing lean-ctx:
lean-ctx benchmark run .           # Human-readable output
lean-ctx benchmark run . --json    # Raw JSON data (per-file, per-mode, tiktoken counts)
lean-ctx gain                      # CLI compression stats (cumulative production usage)
lean-ctx gain --json               # CLI stats as JSON
```

---

## Links

- **GitHub:** [github.com/yvgude/lean-ctx](https://github.com/yvgude/lean-ctx)
- **Install:** `cargo install lean-ctx`
- **Version:** 3.6.6

---

*Measured 2026-05-18 on lean-ctx 3.6.6 against the lean-ctx codebase itself (Rust/TS/Python, ~50K LOC). All token counts from tiktoken cl100k_base (exact), not character-based estimates. Benchmark fix applied: modes returning 0 tokens excluded from "best" ranking — 0 output is data loss, not compression.*
