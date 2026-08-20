# v1.0 Community Activation — Discord, Supporter Mail, Release Notes

> Publish-ready copy for T-0 per `docs/releases/v1.0-runbook.md`.
> Send order: Release notes (with the tag) → Discord @everyone (09:45 CET,
> after blog is live) → Supporter mail (10:00 CET) → HN 15:00 CET.

---

## 1. Discord announcement (#announcements, @everyone)

**lean-ctx 1.0 is out.**

One promise, mechanically enforced: **your setup will not break.**

- 29 protocol contracts classified frozen/stable/experimental — the frozen
  ones are SHA-256-locked in CI. A PR that touches a frozen surface fails.
- The `/v1` HTTP API can only grow. Removals fail a snapshot test.
- Python/TS/Rust SDKs prove conformance against a real engine build on every
  commit. The release pipeline refuses to ship an engine the SDKs can't speak to.

Already running 0.x? The whole migration:

```
lean-ctx doctor --migrate-check
```

It will tell you what we already know: nothing to do.

Blog: https://leanctx.com/ · Contracts: https://github.com/yvgude/lean-ctx/blob/main/CONTRACTS.md · Press kit: https://leanctx.com/press/

If 1.0 is useful to you, today is the day an upvote helps the most —
Show HN goes up at 15:00 CET (link follows in #general). One ask, as always:
**only vote if you genuinely use it.** No vote rings — HN notices, and so do we.

---

## 2. Supporter mail (sponsors + supporters wall, BCC)

**Subject:** lean-ctx 1.0 — you made this independent

Hi,

lean-ctx hit 1.0 today. Before anything public goes up, you hear it first —
because this release exists in large part thanks to you.

What 1.0 means in one line: every surface you build on is now contractually
stable — 29 documented contracts, the critical ones hash-locked in CI, an
HTTP API that can only grow, SDKs that prove conformance on every commit.
Your integrations cannot silently break. That promise is the release.

What stays exactly the same: local-first, zero telemetry, Apache-2.0, every
local feature free forever. That is now a frozen contract too
(`local-free-invariant`), not just a sentence in a README.

If you have 5 minutes today:

1. Run `lean-ctx doctor --migrate-check` on your install — if anything is
   not green, reply to this mail and it becomes today's top priority.
2. The Show HN goes live at 15:00 CET. If lean-ctx earns it, your voice in
   the comments (real usage, real numbers) is worth more than any upvote.

Thank you for keeping a one-person project independent.

— Yves
https://leanctx.com/support/ · press kit: https://leanctx.com/press/

---

## 3. Release notes — founding-user thanks (append to v1.0.0 notes)

## Thank you

1.0 is a promise, and promises need witnesses. To everyone on the
[supporters wall](https://leanctx.com/support/), every sponsor, every person
who filed an issue, challenged a benchmark, or ran an RC build through their
real workload: you are the reason this project could afford to freeze its
contracts instead of chasing features. The first names on the wall were here
before there was anything to gain from it — **founding users** in the truest
sense. This one is yours.

---

## Timing & ownership (from the runbook)

| When (CET) | What | Owner |
|---|---|---|
| 09:00 | Tag + release notes live | maintainer |
| 09:30 | Blog post live | maintainer |
| 09:45 | Discord announcement | maintainer |
| 10:00 | Supporter mail | maintainer |
| 15:00 | Show HN + Discord #general link | maintainer |
| 15:00–23:00 | Q&A window (HN/Discord/PH) | maintainer |
