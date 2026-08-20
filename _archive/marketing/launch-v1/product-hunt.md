# Product Hunt — v1.0 Launch Draft

> Launch T+1 after Show HN (deliberately offset — two separate traffic spikes,
> and the HN thread becomes social proof in the PH comments).
> Copy rules: `marketing/submissions.md`.

## Tagline (60 chars max — pick one)

1. `The Context OS for AI development` *(canonical narrative)*
2. `Cut your AI coding agent's token bill by up to 99%`
3. `Local context layer for Cursor, Claude Code & 24+ tools`

Default: #1. Use #2 if the gallery leads with the savings dashboard.

## Description (260 chars)

LeanCTX is a local Rust binary between your AI agent and your codebase:
compressed reads, cached context, semantic search. 79 MCP tools, zero
telemetry, Apache-2.0. v1.0 freezes 29 protocol contracts — your setup
can't break. Up to 99% token savings.

## First comment (founder)

Why 1.0 now: stability is the feature. 29 contracts
frozen/stable/experimental, CI rejects any change to a frozen surface, three
SDKs prove conformance on every commit, and `lean-ctx doctor --migrate-check`
shows on your machine that upgrading needs zero steps. Ask me anything —
including where lean-ctx does *not* help (we document that too).

## Gallery (in order)

1. Dashboard with live token-savings counters (hero)
2. Before/after: raw `cat` vs `ctx_read` map mode on the same file
3. Savings heatmap (`ctx_heatmap`)
4. Wrapped/recap card (shareable proof)
5. Contract stability matrix (CONTRACTS.md table screenshot)
6. 30-sec setup: `curl … | sh && lean-ctx setup`

## Hunter & logistics

- Self-hunt from the maker account; hunter outreach only if a top-50 hunter
  responds before RC week (do not delay launch for a hunter).
- Schedule 00:01 PT. Maker availability: full launch day, CET evening covered.
- Topics: Developer Tools, Artificial Intelligence, Open Source.
- Link: `https://leanctx.com?utm_source=ph&utm_campaign=v1-launch`.

## Community activation (launch day)

- Discord announcement with direct PH link (no vote-begging language —
  "we're live, answering questions" framing).
- Supporter mail (founding-user thanks + PH/HN links) via the existing list.
- Reply to every PH comment within 2 hours during launch day.
