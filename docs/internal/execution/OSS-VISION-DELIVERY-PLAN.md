# OSS Vision Delivery Plan

**Status:** active implementation plan
**Updated:** 2026-08-22
**Scope:** the non-website, OSS-authorized Context SDK vision only

## Authority and boundary

This plan is governed by `docs/internal/README.md`, then Context SDK
Positioning, Product Architecture, technical truth, and the Anti-Roadmap.
The Master Execution Plan remains the status tracker.

LeanCTX is the local-first Context SDK around an existing agent loop:
**Select → Shape → Reuse → Recover**. The host owns agent logic, model,
prompts, tools, retries, scheduling, permissions, business logic, and UX.

The following work is deliberately excluded:

- Website work.
- WS-9 / Thinkery control plane, tenant data, hosted operation, learned
  selection, billing, and commercial pilot delivery; those require the
  designated private repository, infrastructure, and customer authority.
- New marketplaces, registries, package formats, agent builders, schedulers,
  generic orchestration, or remote coordination products.

Existing local session, knowledge, handoff, receipt, and evidence substrate is
protected. Existing coordination/network surfaces are contained and deprecated;
they are not expanded.

## Delivered foundations

| Workstream | Delivered | Remaining boundary |
|---|---|---|
| W0 evidence contract | Additive `evidence-bundle-v2` prose, strict schema, canonical golden fixtures, and Rust schema/invariant test. | No V2 producer, independent verifier, or public claim path exists yet. |
| W2 offline fixtures | Suite/lock/clone/scorer path and symlink boundaries are fail-closed; the committed offline testbench remains deterministic. | Resource caps, cross-platform executor parity, and provider-free study convergence remain open. |
| W3 Python foundation | SDK endpoint resolution matches Runtime configuration semantics; health probes are token-free; POC doctor observes the real endpoint. | Agents lifecycle proof, repair workload, clean-machine quickstart, and Attach release matrix remain open. |

## Delivery rules

1. One canonical contract per concern; adapters preserve compatibility but do
   not silently create parallel semantics.
2. No claim is promoted by code presence. Public evidence requires a declared
   workload, matched arms, declared quality, provenance, and independently
   runnable verification.
3. A value is measured, calculated, estimated, unavailable, or degraded—never
   silently coerced into another state.
4. Every mutating activation is content-addressed, trusted, explicit, and
   reversible. A failed activation leaves the prior state intact.
5. Every execution boundary is bounded, isolated, deterministic where claimed,
   and fail-closed for trust, identity, path, quality, or resource violations.
6. Each wave owns a narrow file set, has migration fixtures, and passes its
   quality gates before the next wave consumes it.

## Ordered workstreams

### W0 — Freeze contracts and status language

**Goal:** make the evidence, SDK, Profile/Kit, and selection boundaries
unambiguous before code migration.

- Freeze audit `evidence-bundle-v1` as its existing audit-chain contract.
- Introduce additive, customer-proof `evidence-bundle-v2` vocabulary; one
  canonical ID, canonical JSON, digest syntax, signature encoding, trusted key
  path, inventory rule, redaction class, cost state, and claim-validity result.
- Freeze `manual-selection-v1`, `benchmark-comparison-evidence-v1`,
  `tuning-profile-v1`, and `context-kit-v1` contract schemas plus
  Rust-produced golden fixtures for independent consumers.
- Reconcile documentation that treats research-only A2A/coordination as stable
  product availability; label all current capability states accurately.

**Exit:** schemas reject unknown/ambiguous data; golden vectors cover canonical
bytes, IDs, hashes, signatures, and compatibility projections.

### W1 — Reproducible evidence and independent verification (WS-7)

**Goal:** one local, customer-safe proof bundle with a verifier independent of
the producer.

- Add a single receipt recorder/assembler with terminal execution state,
  task/run/profile/kit/fixture/runtime identity, exact provider facts, signer
  key ID, and durable idempotency.
- Assemble matched baseline/treatment evidence through one path: methodology,
  spec, invocation controls, output/replay class, evaluator evidence, pricing
  snapshot, receipts, redaction policy, chain/checkpoint, inventory, and
  trusted signature metadata.
- Make `leanctx-verify` verify V2 structure, canonical bytes, duplicate/unsafe
  archive entries, signer trust, receipt semantics, task/profile/control joins,
  integer arithmetic, quality eligibility, replay class, and claim validity.
- Route report/export/customer-facing evidence commands through independent
  verification; invalid or self-attested-only bundles cannot make proof claims.

**Exit:** Rust producer and standalone verifier consume the same golden bundles;
all tamper, trust, cost, quality, matching, and determinism mutation tests fail
closed.

### W2 — Bounded offline workload and fixture foundation (WS-7)

**Goal:** named proof workloads are portable, local-first, and safe to run.

- Define one offline mode using a versioned manifest, content-addressed fixture,
  suite, recording, evaluator, platform/executor capability, and strict replay.
- Use one bounded evaluator/executor boundary. Reject absolute/escaping/symlink
  paths, malformed rows, oversized manifests/output, excessive repeats/timeouts,
  and provider/network fallback in offline mode.
- Separate dynamic observables from canonical evidence. Record stable repeat
  identifiers plus fixture/evaluator/runtime fingerprints.
- Give each supported platform an explicit executor contract; unsupported modes
  produce no evidence rather than a weakened sandbox.

**Exit:** committed fixture replay works on each claimed platform; path,
resource, output-flood, timeout-descendant, content-drift, and replay-miss
tests are green.

### W3 — Python v1 reference proof loop and Attach gates

**Goal:** a clean developer can prove one real SDK workflow, then roll it back.

- Correct `LeanCTX()` endpoint discovery and align package metadata, support
  matrix, licensing/status text, package name, and pinned dependencies.
- Make OpenAI Agents support a narrow, release-gated contract: one root task,
  one correlated session/receipt, ordered observations, exact stream/cancel/
  exception handling, and no implicit global latest session.
- Reuse the deterministic code-repair fixture for the only v1 reference agent:
  matched stock/treatment arms, real patch application, declared tests, hashed
  inputs, provider facts, Receipt-linked comparison, secret/redaction guard,
  and offline verification.
- Make 15-minute quickstart, doctor, uninstall/rollback, and post-rollback
  stock run deterministic release gates. Keep Codex, Claude Code, and Cursor
  Attach status/install/doctor/uninstall/rollback smoke-tested and truthful.

**Exit:** clean-environment CI runs required Agents integration tests (no
`importorskip` pass), quickstart and two failure/rollback rehearsals succeed,
and all claims point to verified W1 evidence.

### W4 — Manual selection with external trust (WS-8)

**Goal:** evaluated evidence can produce an explainable, reversible local
recommendation without automatic promotion.

- Require V2 independently verified comparison bundles and an out-of-band
  trusted signer before recommendation or apply.
- Bind every candidate, task, profile/kit digest, workload/control identity,
  evaluator result, rationale, and economics value to signed arm metadata.
- Store canonical, nonempty selection IDs and immutable previous/selected
  profile pins. Apply verifies first, uses compare-and-swap state, exposes
  dry-run/trust status, and makes rollback idempotent.
- Provide standalone-verifier and second-implementation schema/golden
  conformance fixtures; mutate every acceptance/rejection boundary.

**Exit:** selection cannot apply self-signed, foreign, stale, incomplete,
noncanonical, changed-profile, or semantically mismatched evidence; failed
operations leave local configuration unchanged.

### W5 — Additive Context SDK, Profiles, and Kits

**Goal:** promote the minimal, language-neutral SDK contract only after its
proof path exists.

- Add protocol-owned typed contracts for `ContextSession`, source, view,
  project context, handoff, policy, receipt, package, `TuningProfileV1`, and
  `ContextKitV1`; use bounded opaque IDs, SemVer, and `sha256:<hex>` identity.
- Resolve legacy TOML, snapshots, and `.ctxpkg` once at the Runtime boundary
  into immutable pinned handles. Keep old readers/adapters but never emit a
  legacy identity as V1.
- Activate profiles/kits only after canonical integrity, compatibility, and
  optional trust verification. Preserve a verified previous pin for atomic
  rollback; same logical identity with another digest is a collision.
- Thread exact pins through session acknowledgement, one session-owned receipt
  assembler, Python bindings, and verified evidence. Existing low-level Rust
  engines/clients remain lower-level APIs, not a second public facade.

**Exit:** Rust/Python golden parity covers hashes, source lineage, activation,
scope/privacy, degradation, tampering, collision, migration, and rollback.

### W6 — Contain non-SDK coordination/network surfaces

**Goal:** protect local substrate while removing accidental platform reach.

- Deprecate session task/workflow/agent public advertisement, agent-card
  discovery, remote transport configuration, and non-core dashboard panels.
- Keep dashboard loopback/authenticated. Split network routers and optional
  dependencies from local stdio MCP/CLI paths, then validate the local SDK
  builds without them.
- Fix local presence persistence before relying on it: production reaper,
  throttled independent heartbeat, atomic write/lock/recovery, instance nonce,
  hard resource bounds, project-scoped scratchpad/facts, typed mutation errors,
  and truthful handoff delivery status.
- Preserve read/export/migration paths for existing task/workflow data during a
  compatibility window; quarantine, never silently delete, old local state.

**Exit:** no remote/discovery claim survives; local ContextSession, knowledge,
handoff, receipts, and evidence work without coordination/network routers; all
liveness, isolation, persistence, and drain tests pass.

### W7 — Release, documentation, and claim gate

**Goal:** ship only what the implementation proves.

- Required CI: Rust format/clippy/lib + targeted integration tests; standalone
  verifier; Python supported-version/Agents matrix; package asset parity;
  clean install/doctor/quickstart/rollback; cross-platform offline fixtures;
  deterministic golden/conformance suites.
- Reconcile master status, positioning/status map, package documentation, and
  generated integration matrices with test-backed capability state.
- Publish no website changes and no paid/private claims from this repository.

**Exit:** release artifacts include all contracts/assets/verifier, every public
claim has a reproducible evidence link and limitation, and the worktree is
clean after the quality gates.

## Delivery checkpoints

| Checkpoint | Required proof |
|---|---|
| C1 | W0 contracts + golden vectors accepted by producer and standalone verifier |
| C2 | W1–W2 clean offline baseline/treatment bundle independently verified |
| C3 | W3 Python repair quickstart + rollback drill on clean environment |
| C4 | W4 selection apply/rollback from independently verified evidence |
| C5 | W5 cross-language SDK/Profile/Kit conformance and immutable pins |
| C6 | W6 local-only SDK surface and reliable bounded local substrate |
| C7 | W7 release gates, status/docs reconciliation, signed delivery evidence |

No checkpoint may be marked complete from a smoke test, self-attestation, code
presence, or an aggregate metric alone.
