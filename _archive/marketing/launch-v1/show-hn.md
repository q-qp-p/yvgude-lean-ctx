# Show HN — v1.0 Launch Draft

> Posting window: launch day 15:00 CET (≈ 09:00 US-East). Founder account, no
> UTM on the main link. Stay in the thread for at least 8 hours.
> Copy rules: `marketing/submissions.md` (LeanCTX in prose, `lean-ctx` in code).

## Title (pick one, A/B against char limit 80)

1. `Show HN: LeanCTX 1.0 – a local context layer that cuts agent token use up to 99%`
2. `Show HN: LeanCTX 1.0 – Context OS for AI coding agents (Rust, local, Apache-2.0)`
3. `Show HN: I froze 29 protocol contracts so my AI tool can't break your setup`

Pick #1 unless the thread climate that week favors the contrarian angle (#3).

## Body

LeanCTX is a single local Rust binary that sits between your AI coding agent
(Cursor, Claude Code, Codex CLI, 24+ tools) and your codebase. It replaces raw
file reads, shell output and search with compressed, cached, structured
context: 79 MCP tools, 10 read modes, 95+ shell compression patterns. Cached
re-reads cost ~13 tokens. Everything runs locally — zero telemetry, no cloud
dependency, Apache-2.0.

What 1.0 actually means (and why it took this long):

- 29 protocol contracts classified frozen/stable/experimental; the seven
  platform promises are SHA-256-frozen in CI — a PR that touches a frozen
  contract file fails the build.
- The public /v1 HTTP API can only grow: a snapshot test rejects removals and
  auth changes.
- Three SDKs (Python/TypeScript/Rust) each pass a 14-check conformance kit
  against the engine in CI; the release pipeline refuses to ship an engine an
  SDK can't speak to.
- `lean-ctx doctor --migrate-check` proves on YOUR machine that 0.x → 1.0 has
  zero migration steps.

Benchmarks with reproducible scripts and a signed scorecard are in the repo —
including the configurations where lean-ctx does NOT help (tiny repos,
one-shot questions), because context layers are not magic.

Repo: https://github.com/yvgude/lean-ctx
Docs: https://leanctx.com

I'll be here all day — happy to go deep on the compression mechanics, the
contract freeze, or why this is a layer and not a fork of your agent.

## Prepared Q&A (tokbench learnings, GL #308/#361)

| Expected question | Answer skeleton |
|---|---|
| "Benchmark X shows worse numbers" | Ask for their config first. Most external runs miss the bridge mode (`LEAN_CTX_PI_ENABLE_MCP=1`) and measure envelope overhead as if it were payload. Link the savings-faithful config doc + offer to re-run their exact scenario. |
| "Isn't this just prompt caching?" | Caching dedupes identical prefixes at the provider. LeanCTX changes *what* enters the window: AST-aware read modes, compressed shell output, semantic dedup — works across providers and with local models. The two stack. |
| "99% is marketing" | "Up to" is load-bearing and the scorecard is signed + reproducible. Typical sessions: 60–90%. The 99% case (cached re-read, 13 tokens vs full file) is real and documented. Show the per-tool cost table. |
| "Lock-in?" | Apache-2.0, local-first, no account. Remove it and your agent still works — just spends more tokens. The contract freeze is exactly the anti-lock-in: your integration cannot break within v1. |
| "Why Rust / why a binary?" | Tree-sitter for 18 languages, single static binary, no Python env conflicts with the tools it wraps, sub-ms hot paths for shell interception. |
| "Privacy?" | Zero telemetry, all local. The optional cloud sync is opt-in, scoped, and documented. Point to the local-free-invariant contract (frozen). |
| "Does it work with [tool]?" | 24+ documented integrations; three modes (CLI redirect / hybrid / full MCP). If their tool speaks MCP or a shell, yes. |

## Don'ts

- No employee/friend upvote rings — HN detects voting anomalies.
- Never edit the post after traction; clarify in comments.
- No links behind signups. No "DM me".
- Concede valid criticism fast and concretely; commit to a fix with an issue link.
