# Master Execution Plan

**Status:** Active — single source of truth for all execution  
**Owner:** Yves Gugger  
**Created:** 2026-08-20  
**Updated:** 2026-08-22
**Tracking:** GitLab issues on `origin` (project ID 5, `gitlab.pounce.ch/root/lean-ctx`)

---

## Workstreams (parallel)

```text
WS-1: CODEBASE CLEANUP          ██████████  DONE (2026-08-20)
WS-2: SDK V1 + EVIDENCE         ██████████  Technical v1 complete; commercial sprint separate
WS-3: WEBSITE REBUILD           ████████░░  v2 live, Nav/Footer/SEO/Docs pending
WS-4: OCLA + COMPOSABLE (B)     ██████████  Native + bounded external capability paths complete
WS-5: PARTNERSHIP (C)           —— CANCELLED (monopoly strategy)
WS-6: BENCHMARK + CALIBRATOR    ██████████  Technical v0 complete; no automated selection claims
WS-7: REPRODUCIBLE EVIDENCE (C) ██████░░░░  Manifest and signed-bundle foundations exist; provenance replay and independent verification remain
WS-8: MANUAL SELECTION (D)      ████░░░░░░  Local record/apply/rollback primitives exist; evidence verification and full conformance remain
WS-9: THINKERY CONTROL PLANE(E) ░░░░░░░░░░  Private commercial work; separate repository/infrastructure
```

---

## WS-1: Codebase Cleanup

**Goal:** Repo reflects ONE vision. Remove confusion, consolidate duplicates.  
**Timeline:** This week  
**Safety:** Each wave = 1 branch → 1 PR → gates green → merge

| # | Task | Branch | Status |
|---|------|--------|--------|
| 1.1 | Archive `py-sdk/`, `python-sdk/`, `clients/python/` | `cleanup/wave-1-python` | **done** (f615c8d) |
| 1.2 | Archive `marketing/`, `email-templates/`, `demo/`, `lab/`, `blog/`, `lean/` | `cleanup/wave-2-unused` | **done** (6441f73) |
| 1.3 | Consolidate `bench/` + `benchmark/` → `benchmarks/` | `cleanup/wave-3-bench` | **done** (6679c89) |
| 1.4 | Remove `test-results/`, `tmp/` from tracking | `cleanup/wave-3-bench` | **done** (6679c89) |
| 1.5 | Update README.md links after cleanup | main | **done** (2026-08-21: Pi-install lines removed) |
| 1.6 | Prio 1: DELETE ts-sdk, MERGE contracts/examples, .gitignore fix | main | **done** (8ea700f) |
| 1.7 | Prio 2+3: Archive editors/go-sdk/integrations/specs/benchmarks | main | **done** (6ff1e48) |
| 1.8 | Prio 4+5: Archive stale scripts, keep bin/lctx | main | **done** (235d135) |
| 1.9 | Sync GitLab WS-1 issues + `_archive/` restoration README | — | **done** (2026-08-21: #1242–#1244 closed, milestone closed) |

**Gates per wave:**
```bash
cargo build --release
cargo test --lib
cargo clippy --all-features -- -D warnings
cargo fmt --check
```

---

## WS-2: SDK v1 + Evidence (Phase A)

**Goal:** One workload, baseline/treatment, quality gate, and offline-verifiable Receipt; commercial conversion runs in parallel, not as an engineering prerequisite.
**Timeline:** 2-3 weeks after cleanup  
**Exit criterion:** Reproducible local Receipt with offline verification

| # | Task | Status |
|---|------|--------|
| 2.1 | Consolidate Python types from `py-sdk/` + `clients/python/` into `packages/python-lean-ctx/` | **done** (pre-existing) |
| 2.2 | Implement `ctx.wrap()` — OpenAI Agents SDK adapter through proxy | **done** (wrap.py 297 LOC) |
| 2.3 | Implement `ContextSession` with correlated task identity | **done** (session.py 321 LOC) |
| 2.4 | Implement Receipt generation on every wrap() call | **done** (receipt.py 410 LOC) |
| 2.5 | Implement local Performance Benchmark (baseline vs treatment) | **done** (evidence realworld) |
| 2.6 | Implement Quality Gate (automated assertions + human gate) | **done** (--quality-gate flag) |
| 2.7 | Implement offline Receipt verification | **done** (verify() local+Ed25519) |
| 2.8 | Write Quickstart documentation | **done** (README 342 LOC) |
| 2.9 | Publish v1.0 on PyPI | **done** (lean-ctx-python 1.0.0) |
| 2.10 | Run first Thinkery Agent Tuning Sprint (CHF 7,500) | **ON HOLD** (commercial track; #1254; does not block vision engineering or public Research releases) |

---

## WS-3: Website Rebuild

**Goal:** New website reflecting the professional brand narrative.  
**Timeline:** Parallel to WS-1/WS-2, 1-2 weeks  
**Branch:** `deploy` (GitLab only, NEVER push to GitHub)  
**Tech:** Astro (existing stack)  
**Copy source:** `docs/internal/execution/WEBSITE-REDESIGN.md` (839 lines, complete)

| # | Task | Status |
|---|------|--------|
| 3.1 | Design system: implement Visual Brand Guidelines (colors, typography, grid) | **done** (4579d29, deploy) |
| 3.2 | Homepage rebuild: Hero + Problem + SDK + Integration + Evidence + Enterprise | **done** (82ebb95, deploy) |
| 3.3 | `/sdk` page: Developer-focused, code examples, progressive disclosure | **done** (7a9fa82, deploy) |
| 3.4 | `/enterprise` page: Performance-first enterprise narrative | **done** (7a9fa82, deploy) |
| 3.5 | `/benchmark` page (replaces old Dyno language) | **done** (7a9fa82, deploy) |
| 3.6 | `/docs` restructure: align with new terminology | pending |
| 3.7 | Remove old pages that don't align (old pricing, old cloud references) | pending |
| 3.8 | Navigation update: LeanCTX / SDK / Enterprise / Benchmark / Docs / GitHub | pending |
| 3.9 | Footer: Open source · Local-first · Model-agnostic | pending |
| 3.10 | SEO: meta titles/descriptions with new messaging | pending |
| 3.11 | Deploy to production (GitLab CI → origin only) | pending |

**Design principles:**
- Black ground, minimal orange (= intervention), Aeonik + Mono
- Routing lines, grids, data panels, receipt visualizations
- No fake metrics — only show real data or clearly labeled illustrations
- Product status labels: Available / Preview / Research

---

## WS-4: OCLA + Composable Architecture (Phase B)

**Goal:** First native capability through the full contract path.  
**Timeline:** Engineering now (owner decision 2026-08-22); public claims require reproducible, independently verifiable evidence, not a paid run.
**Prerequisite for code:** none — dogfood internally as Preview/Research.
**Open-core:** Class A/B only (manifest, registry, native CompressionProvider, conformance). No marketplace, no learned ranking, no Control Plane (Class D/E).

| # | Task | Status |
|---|------|--------|
| 4.1 | OCLA audit verified (D4 report already done) | done |
| 4.2 | Define CapabilityManifest v0 schema in `lean-ctx-protocol` | **done** (634dedc) |
| 4.3 | Normalize `CompressionProvider` as first v0 capability | **done** (634dedc) |
| 4.4 | Add capability ID + version to Performance Profile format | **done** (634dedc) |
| 4.5 | Add capability ID + version to Receipt schema | **done** (634dedc) |
| 4.6 | Run Benchmark through capability path (same result, new plumbing) | **done** (634dedc) |
| 4.7 | Write conformance test for `compression_provider` | **done** (634dedc) |
| 4.8 | Sample external local-process capability (trivial example) | **done** (discovery, fixed executable boundary, bounded stdio, timeout/disable, registry + conformance; 10,199 Rust tests and 3 cookbook tests) |

---

## WS-5: ~~Partnership + Ecosystem~~ CANCELLED

**Decision (2026-08-21):** Gestrichen. LeanCTX strebt Monopolstellung an —
absolute Performance aus einer Hand statt composable Partner-Oekosystem.
Die technische Faehigkeit (OCLA external capability path) existiert als
Fallback fuer Enterprise-Kunden, wird aber nicht aktiv beworben oder
entwickelt.

**Reasoning:**
- Partner-Oekosystem gibt Wertschoepfung ab
- "Composable Layer" positioniert LeanCTX als Infrastruktur statt Loesung
- Monopol auf Token-Savings-Kette ist strategisch staerker
- Kapazitaet besser investiert in eigene Performance-Verbesserungen

**Was bleibt:**
- WS-4.8 External Capability Cookbook (technische Referenz)
- OCLA Contract (Enterprise kann eigene Capabilities bauen falls noetig)
- Kein aktives Marketing, keine Partner-Akquise, kein Oekosystem-Bau

## WS-6: Benchmark + Calibrator (Research Track)
**Goal:** Validate the Calibrator concept: one agent, two profiles, controlled benchmark, quality preserved, correct recommendation.
**Timeline:** After WS-4 OCLA dogfood; Research status until Phase D evidence gate
**Prerequisite:** WS-4 AdapterRegistry production-wired; PerformanceProfileV1 with capabilities field
**Vision:** [Calibration & Performance Platform Vision](../vision/14-LEANCTX-CALIBRATION-PERFORMANCE-PLATFORM-VISION.md)
**Gap Analysis:** [Benchmark & Calibrator Gap](../reference/BENCHMARK-CALIBRATOR-GAP.md)
**Open-core:** Manual calibration = OSS; automated candidate generation + Selection Intelligence = commercial

| # | Task | Status |
|---:|---|---|
| 6.1 | Consolidate 6 benchmark engines under unified BenchmarkSpecV1 | **done** (BenchmarkSpecV1, BenchmarkRunner trait, report formatters; 7 tests) |
| 6.2 | Extend Profile with constraints (quality_floor, max_cost, max_latency) | **done** (ConstraintsConfig + CapabilitiesConfig in Profile; 64 profile tests pass) |
| 6.3 | Extend Profile with capabilities section (surface to provider) | **done** (CapabilityBinding + CapabilitiesConfig) |
| 6.4 | Create performance-profile-v1 contract and JSON schema | **done** (docs/contracts/performance-profile/) |
| 6.5 | Implement benchmark with profile selection (wire profile to benchmark) | done |
| 6.6 | Implement Calibrator v0: fixed candidate set, Pareto frontier, recommendation | **done** (calibrator module: config, candidate, pareto, recommendation, report; 13 tests) |
| 6.7 | Implement calibrate CLI command | done |
| 6.8 | Agent Connector v0: programmatic invocation of one agent for benchmark | **done** (AgentConnector trait + Codex/Claude/Cursor connectors + detection; 3 tests) |
| 6.9 | LocalRunner wiring and named-profile propagation for live calibration | **done** (LocalRunner, timeout, and `LEAN_CTX_PROFILE` propagation) |
| 6.10 | Local verified comparison artifact: explicit quality evaluator + Receipt linkage | **done** (deterministic evaluator + `--spec` gate + canonical locally signed connector receipt from explicit provider usage/cost; a connector without explicit cost remains correctly OBSERVED) |

**Anti-scope:** No gamification, social profiles, achievements, badges, community platform, marketplace. V1 = one agent, two profiles, controlled benchmark, Receipt, recommendation.

## WS-7: Reproducible Evidence (Phase C, OSS)

**Goal:** Make the Phase A/B primitives proveable over named, evaluated workloads without
turning a local benchmark into a hosted ranking service.
**Status:** Research. A paid run is not required; public claims need a reproducible workload,
predeclared quality gate, complete provenance, and independently runnable verifier.
**Open-core:** Class A/B/C: workload and evidence contracts, local runner, reference fixtures,
report formatter, offline verifier. No learned ranking, fleet telemetry, customer data, or
hosted history.

| # | Task | Exit criterion | Status |
|---:|---|---|---|
| 7.1 | Versioned evaluated-workload manifest | Stable identity, declared QA/code evaluator, bounded code-test fixture, deterministic validation | in progress — versioned source-probe and code-repair manifests exist; hardened bounded fixture execution remains required |
| 7.2 | Local suite loader and named-suite CLI | `benchmark-run --profile NAME --spec PATH` executes an evaluated manifest with explicit profile/agent identity | **done** (strict JSON loading, evaluator gate, deterministic profile binding, no `--suite`/`--repeats` overrides) |
| 7.3 | Reproducible evidence bundle | Baseline/treatment outputs, evaluator result, receipt refs, artifact redaction classification, environment and verifier command are linked offline | in progress — signed local spec/result/receipt bundle and explicit redaction classification exist; invocation binding, output replay, and independent receipt/evidence verification remain required |
| 7.4 | Capability coverage matrix | Native and bounded external capability paths show success, policy rejection, timeout, and disable behavior | in progress — deterministic, payload-free test matrix covers each state; a consumable evidence surface remains required |
| 7.5 | Public research fixture pack | Redacted/self-contained fixtures pass on a clean checkout and make no universal-performance claim | in progress — manifests/assets are portable and provider-free; isolated code-repair proof currently requires the macOS sandbox, so cross-platform proof remains open |

## WS-8: Manual Selection (Phase D, OSS)

**Goal:** Convert evaluated local evidence into an explainable, reversible manual recommendation.
**Status:** Research/Preview only after WS-7 evidence exists.
**Open-core:** Class A/B/C deterministic candidate generation, Pareto calculation, explicit
operator selection, exported profile, and rollback. Learned rankings, customer priors, and
automatic promotion stay private Class D/E.

| # | Task | Exit criterion | Status |
|---:|---|---|---|
| 8.1 | Evidence-qualified candidate input | Unevaluated or incomplete-cost runs cannot feed a recommendation | in progress — creation validates evaluated receipt-linked runs; independent evidence verification is still required before later apply |
| 8.2 | Deterministic recommendation record | Candidate set, constraints, evidence refs, rationale, and profile hash serialize canonically | in progress — canonical serialization exists; its linked evidence requires independent validation |
| 8.3 | Explicit apply/rollback CLI | Operator approves a named profile; prior profile is preserved and restorable | in progress — record/apply/rollback supports later-record apply and refuses stale state; independent evidence verification remains required |
| 8.4 | Manual-selection conformance suite | Stable result across reordered inputs; all rejection paths are covered | in progress — ordering, later apply, stale state, and immediate rollback are covered; tampered/unavailable-evidence rejections remain |

## WS-9: Thinkery Control Plane (Phase E, Commercial, private)

**Goal:** Govern continuous optimization safely for organizations without exporting their
private workloads, rankings, prices, or policy data into the OSS repository.
**Repository boundary:** This work belongs in a separate private Thinkery repository and
private infrastructure. It must not be scaffolded as a hidden feature in LeanCTX OSS.

| # | Capability | Boundary | Status |
|---:|---|---|---|
| 9.1 | Organization registry, access control, audit and retention | Class D; private | blocked — private project not designated |
| 9.2 | Scheduled experiment queue, budget, approval, rollback | Class D; private | blocked — private project not designated |
| 9.3 | Learned ranking, customer priors, continuous optimization | Class E; private | blocked — governed data and private project not designated |
| 9.4 | Fleet roll-out/canary and support/SLA operation | Class D/E; private | blocked — private infrastructure not designated |


## Tracking Rules

1. **Each task gets a GitLab issue** with label `status: ready` or `status: in-progress`
2. **Each workstream gets a GitLab milestone** (WS-1, WS-2, WS-3, WS-4, WS-5)
3. **PR links to issue** — close on merge (or when published, for SDK/website)
4. **Weekly status update** in this document
5. **No task without exit criterion** — what does "done" look like?

### GitLab sync (2026-08-21)

| Milestone | Issues | State |
|---|---|---|
| WS-1 Codebase Cleanup | #1242–#1244 | **closed** + milestone closed |
| WS-2 SDK v1 + Evidence | #1245–#1253 | **closed** (code + PyPI 1.0.0) |
| WS-2 | #1254 first paid pilot | **open — ON HOLD** (sales/pilot) |
| WS-3 Website Rebuild | #1255–#1259 | **closed** (v2 pages on `deploy`) |
| WS-3 | #1260 old pages / nav / footer / SEO | **open** |
| WS-3 | #1261 production deploy | **open** |
| WS-4 | #1262–#1266 | **open** — engineering unblocked 2026-08-21; do not market |
| WS-5 | #1267–#1268 | **cancelled** — monopoly strategy, close issues |

---

## What we are NOT doing (Anti-Roadmap reminder)

- No marketplace
- No 10 partner integrations
- No partner ecosystem or "composable optimizer" marketing
- No RTK/Headroom/Caveman integrations or joint experiments
- No OptimizationProvider interop promotion
- No control plane or dashboard
- No AutoTune (Continuous Optimization is Phase E, later)
- No hosted platform claims
- No new package formats
- No agent builder

---

## Success Definition

```text
✓ Repo is clean (one Python SDK, no dead dirs)
✓ Website reflects the new narrative
✓ SDK v1 on PyPI with ctx.wrap() + Receipt
✓ One paid customer has a verified Receipt
✓ CompressionProvider runs through OCLA v0 contract
✗ WS-5 cancelled — monopoly strategy, no external partnerships
```

When all six are true, we have earned the right to talk about
"Context Performance Infrastructure" publicly.
