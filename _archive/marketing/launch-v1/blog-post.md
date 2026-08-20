# lean-ctx 1.0: Stability as a Feature

> Launch-day announcement post — publish-ready. Goes live at T-0 09:30 CET per
> `docs/releases/v1.0-runbook.md` (website blog route or GitHub Release notes +
> linked from HN/PH). EN only at launch; translations follow.

---

Today lean-ctx hits 1.0. Not because we ran out of version numbers below one —
the engine internally shipped hundreds of releases — but because we can now
make a promise we could not make before: **your setup will not break.**

## What 1.0 means, mechanically

Most 1.0 announcements are a feeling. This one is a set of failing CI jobs
waiting to happen:

- **29 protocol contracts, classified.** Every surface lean-ctx exposes — the
  CLI, the 79 MCP tools, the HTTP `/v1` API, the team-server wire protocol,
  the Context IR — is documented in a contract file and classified
  `frozen`, `stable`, or `experimental`. The classification is not prose: it
  lives in `core::contracts` and is served at runtime via
  `GET /v1/capabilities` (`contract_status`).
- **Frozen means frozen.** The seven platform-promise contracts are
  SHA-256-locked in CI. A pull request that edits a frozen contract file fails
  the build. Evolution happens in a new `-v2.md` next to the immutable v1 —
  never in place.
- **The API only grows.** A snapshot test pins every `/v1` route and its auth
  requirement. Additions pass; removals and auth changes fail.
- **SDKs prove conformance on every commit.** The Python, TypeScript, and Rust
  SDKs each run a 14-check conformance kit against a real engine build in CI —
  including a drift gate that fails when the server grows a route an SDK
  doesn't cover. The release pipeline refuses to tag an engine an SDK cannot
  speak to.

## The migration guide is one command

```bash
lean-ctx doctor --migrate-check
```

It audits your config against the schema, checks the deprecation register
(currently empty), verifies your data layout, and confirms the contract set.
On every machine we have run it on, the answer is the same:
**ready for 1.0 — no migration steps required.** That is the entire guide.
([The long version](https://github.com/yvgude/lean-ctx/blob/main/docs/releases/migration-1.0.md)
exists, mostly to show its own emptiness.)

## The numbers, signed

Cached re-reads cost ~13 tokens. Typical sessions save 60–90% of context
tokens, up to 99% in cached workflows — measured locally on your machine
(`ctx_metrics`, `ctx_cost`), never via telemetry. Benchmark claims ship with
reproducible scripts and a cryptographically signed scorecard
([BENCHMARKS.md](https://github.com/yvgude/lean-ctx/blob/main/BENCHMARKS.md)).
We also document where lean-ctx does *not* help: tiny repos, one-shot
questions, contexts that fit comfortably anyway. A context layer is not magic;
it is bookkeeping done so well it looks like magic on your invoice.

## What stays the same

Everything that made lean-ctx what it is, is contractually unchangeable now:

- **Local-first, zero telemetry** — enshrined in the frozen
  `local-free-invariant` contract.
- **Apache-2.0** — fork it, audit it, ship it.
- **Free forever** for every local feature. The paid plane (team workspaces,
  hosted index) lives entirely server-side.

## Thank you

To every supporter on the [wall](https://leanctx.com/support/), every issue
reporter, every benchmark critic who made us sharper: this release carries
your fingerprints. You kept a one-developer project independent long enough to
make promises like these.

Install: `curl -fsSL https://leanctx.com/install.sh | sh` ·
[Migration check](https://github.com/yvgude/lean-ctx/blob/main/docs/releases/migration-1.0.md) ·
[Contracts](https://github.com/yvgude/lean-ctx/blob/main/CONTRACTS.md) ·
[Press kit](https://leanctx.com/press/)
