# Master Execution Plan

**Status:** Active — single source of truth for all execution  
**Owner:** Yves Gugger  
**Created:** 2026-08-20  
**Updated:** 2026-08-21  
**Tracking:** GitLab issues on `origin` (project ID 5, `gitlab.pounce.ch/root/lean-ctx`)

---

## Workstreams (parallel)

```text
WS-1: CODEBASE CLEANUP          ██████████  DONE (2026-08-20)
WS-2: SDK V1 + EVIDENCE         █████████░  v1.0.0 on PyPI; paid sprint **ON HOLD** (#1254 sales/pilot)
WS-3: WEBSITE REBUILD           ████████░░  v2 live, Nav/Footer/SEO/Docs pending
WS-4: OCLA + COMPOSABLE (B)     ████████░░  Engineering dogfood allowed; no public claim until #1254
WS-5: PARTNERSHIP (C)           —— CANCELLED (monopoly strategy)
WS-6: BENCHMARK + CALIBRATOR    ███████░░░  LocalRunner + named-profile comparison implemented; next: quality evaluator + Receipt linkage
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

**Goal:** One workload, baseline/treatment, quality gate, receipt, paid customer.  
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
| 2.10 | Run first Thinkery Agent Tuning Sprint (CHF 7,500) | **ON HOLD** (sales/pilot; #1254) |

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
**Timeline:** Engineering now (owner decision 2026-08-21); public claims still wait for #1254  
**Prerequisite for marketing:** paid Receipt. **Prerequisite for code:** none — dogfood internally as Preview/Research.
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
| 6.10 | Local verified comparison artifact: explicit quality evaluator + Receipt linkage | **in progress** (deterministic evaluator + `--spec` gate + canonical locally signed connector receipt from explicit provider usage/cost; live paid-provider evidence remains) |

**Anti-scope:** No gamification, social profiles, achievements, badges, community platform, marketplace. V1 = one agent, two profiles, controlled benchmark, Receipt, recommendation.


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
