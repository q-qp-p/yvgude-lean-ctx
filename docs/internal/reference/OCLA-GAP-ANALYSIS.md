# OCLA Capability Architecture Gap Analysis

**Status:** WS-4 architecture audit
**Scope:** Current Rust OCLA implementation compared with the canonical `CapabilityManifest v0` architecture
**Decision horizon:** Post-v1 Phase B capability substrate; no broad OCLA or Cargo-workspace refactor is proposed here

## 1. Executive conclusion

LeanCTX has two useful but disconnected OCLA substrates:

1. A **working legacy service registry**: `lean-ctx-ocla` declares `OclaService` plus 15 concrete service traits. `core::ocla::OclaRegistry::global()` instantiates all 15 `Builtin*` implementations and production paths call them.
2. A **manifest-and-adapter path**: `CapabilityManifestV1`, `CapabilityAdapter`, `CapabilityInvocation`, and `AdapterRegistry` provide an early versioned execution shape. It has native, passthrough, and RTK reference adapters, but normal runtime startup neither creates nor populates `AdapterRegistry`.

The migration must converge these substrates. It must not replace the 15-service registry with another registry, infer the v0 taxonomy from the legacy trait count, or reinterpret `CapabilityManifestV1` fields silently.

`BuiltinCompressionProvider` is the right reference capability. It is bounded, local, synchronous, has an existing production call site, and maps directly to the v0 `compression_provider` type. The first end-to-end path should make that existing behavior available through an additive, manifest-backed contract, with the old dispatch retained behind a feature flag until parity is proved.

## 2. Audit evidence and boundary

This analysis compares the canonical architecture in:

- `docs/internal/reference/OCLA-CAPABILITY-ARCHITECTURE.md`
- `docs/internal/reference/OCLA-CURRENT-STATE.md`
- `docs/internal/execution/CONVERGENCE-PLAN.md`

with the compiled Rust surfaces in:

- `rust/crates/lean-ctx-ocla/src/traits.rs`, `types.rs`, and `lib.rs`
- `rust/crates/lean-ctx-protocol/src/capability.rs`
- `rust/src/core/ocla/{mod.rs,registry.rs,invocation.rs,adapters/}`
- `rust/src/core/ocla/builtin/`
- `rust/src/core/conformance.rs` and `rust/tests/ocla_contract_suite_v1.rs`

The audit distinguishes compiled/runtime code from source files that exist but are not exported. It does not recommend changing published crates or moving workspace directories as part of WS-4.

## 3. Target architecture to preserve

OCLA v0 is an open capability contract, not a plugin marketplace or a workload-selection engine. A capability has one primary v0 type, declared operations, bounded contracts, explicit effects, and measurable execution behavior.

The v0 taxonomy contains 14 types:

| v0 type | Responsibility |
|---|---|
| `context_planner` | Select context, representation, budget, and recovery plan. |
| `compression_provider` | Reduce a declared content representation. |
| `shell_output_optimizer` | Shape command, compiler, test, or tool output. |
| `response_optimizer` | Shape MCP, API, or other tool responses. |
| `retrieval_provider` | Supply candidate information without activating it. |
| `knowledge_provider` | Supply provenance-bearing facts or graph knowledge. |
| `memory_provider` | Read/write task or session memory under freshness and policy controls. |
| `context_cache` | Reuse safely scoped material or derived artifacts. |
| `model_router` | Produce a policy-compliant provider/model route. |
| `evaluation_provider` | Assess quality, correctness, or compliance. |
| `outcome_tracker` | Record outcomes that close the optimization loop. |
| `policy_decision_provider` | Decide trust, data, model, and transform policy. |
| `evidence_provider` | Emit receipts, provenance, and independently verifiable evidence. |
| `execution_provider` | Execute or delegate a bounded operation. |

`CapabilityManifest v0` must describe a pinned, bounded, policy-admissible operation: immutable identity, type and operations, exactly one execution mode, input/output contracts, fidelity and recovery, requested permissions, cache semantics, performance limits, trust/privacy boundary, and telemetry. Publisher identity, artifact integrity, license, and manifest signature are additionally required for installable packages.

## 4. Current topology

```text
                         legacy production path

lean-ctx-ocla::traits ──> core::ocla::OclaRegistry::global()
  OclaService + 15          fixed fields; 15 Builtin* services
  service traits                         │
                                      direct callers
                            tools / proxy / lifecycle / CLI

                         manifest-and-adapter path

lean-ctx-protocol::CapabilityManifestV1
                │
     CapabilityAdapter(manifest, invoke, health_check)
                │
      AdapterRegistry(ID, version) ──> adapters
                │                        NativeContext / Passthrough / RTK
                └── construction and registration are test-only

Target: one local capability registry resolves an immutable manifest identity,
admits a scoped policy grant, dispatches an operation, and records the same
identity in Profile, Execution Plan, Receipt, and Benchmark evidence.
```

### 4.1 Working legacy registry

`OclaRegistry` is a global `OnceLock` containing one public `Arc<dyn Trait>` field for every legacy service. `with_builtins()` constructs all 15 builtin implementations. This confirms that the legacy traits are production migration inventory, not dormant interfaces.

The registry has useful properties to retain during transition:

- Explicit native implementations and narrow typed request/result methods.
- Existing production call sites for every service, including `DeliveryRegistry`.
- Object-safe `Send + Sync` trait objects.
- Local, deterministic construction of builtins.

Its shape is not the v0 registry contract:

- One field per trait prevents multiple implementations, operation-based discovery, or independent version selection.
- It has no manifest digest, enabled/disabled state, install source, health record, policy grant, signature state, or conformance result.
- Its global construction is not a runtime catalog and cannot safely represent a third-party local-process capability.

### 4.2 Existing manifest and adapter substrate

The adapter path is worth extending rather than replacing:

- `CapabilityManifestV1` has stable ID, provider, generic kind, version, surfaces, local/remote declarations, reversibility, determinism, data movement, classifications, measurement support, schema references, and a conformance version.
- `CapabilityAdapter` supplies a common `manifest`, `invoke`, and `health_check` boundary.
- `AdapterRegistry` uses deterministic `(capability_id, version)` lookup, rejects duplicates, and offers sorted listing plus health checks.
- `NativeContextAdapter`, `PassthroughAdapter`, and `RtkShellAdapter` implement `CapabilityAdapter` and load pinned JSON manifests.

The substrate is not yet a v0 runtime path:

- `AdapterRegistry::new()` and adapter registration appear only in tests; no normal startup populates it.
- The registry is in-memory only and has no enabled state, lifecycle/audit record, digest collision check, trust verification, policy grant, or disable operation.
- `CapabilityInvocation` carries a task ID, ID/version, one of three input variants, generic constraints, and a timeout. It carries neither a resolved manifest digest, operation ID, permission grant/decision, content/media contract, protected spans, nor receipt lineage.
- `CapabilityResult` contains measurements and references but no typed output payload; an adapter cannot yet act as the canonical data path for a composed operation.
- `PolicyConstraints` declares several values that invocation validation does not fully enforce. Its pre-dispatch check currently constrains context paths and model names; it does not establish scoped OCLA permissions, endpoint authorization, or a recorded policy decision.
- The adapter boundary is synchronous and has no defined cancellation, process isolation, remote transport, or bounded-I/O protocol for the v0 execution modes.

### 4.3 Manifest and conformance state

`CapabilityManifestV1` is a sound compatibility substrate, but its `validate()` checks only schema version and that at least one of `local` or `remote` is true. The unexported `lean-ctx-ocla/src/manifest.rs` performs additional checks, but `lib.rs` does not declare the module and its `semver` dependency is absent from that crate's manifest. It is not a safe module to expose as-is.

`core::conformance::check_manifest_conformance()` and its fixture suite are useful beginnings, but they are test-time checks. They do not validate v0's required shape, are not required during registry admission, and have no type-specific suites. Invocation conformance currently records a latency warning without making it a failing check, so it cannot prove declared performance behavior.

## 5. Current trait inventory and migration status

All 15 concrete traits below extend `OclaService`; all have a `Builtin*` implementation wired by `OclaRegistry::with_builtins()`. The old crate documentation, package description, and CLI status output still say 14 and omit `DeliveryRegistry`; that is documentation/status debt, not a reason to remove a live service.

| Legacy trait | Current production role | v0 mapping | Status and migration action |
|---|---|---|---|
| `ObservationHook` | Emits lifecycle observations from tool metrics paths. | `evidence_provider` | Partial. Fold into receipt/evidence operations; retain as an internal event sink during transition. |
| `UsageSink` | Records proxy usage. | `evidence_provider` | Partial. Unify metrics schema and receipt linkage; do not retain as a top-level v0 type. |
| `MetricsExporter` | Exports bounded local tool-call metrics. | `evidence_provider` | Partial. Become a declared telemetry/evidence operation with delivery policy. |
| `SavingsLedger` | Records read-savings evidence. | `evidence_provider` | Partial. Preserve evidence behavior but consolidate with receipt lineage. |
| `IntentClassifier` | Supplies read-mode/intent classification. | `context_planner` | Partial planner signal. It does not select a complete context representation, budget, and recovery plan. |
| `OutcomeTracker` | Records tool/read outcomes. | `outcome_tracker` | Direct semantic match. Add manifest, operation contract, policy admission, and receipt pinning. |
| `CompressionProvider` | Performs compression projection from the lifecycle path. | `compression_provider` | Direct match and first reference candidate. Connect its existing call through the new registry without changing fail-closed behavior. |
| `ResponseOptimizer` | Records/optimizes response handling after proxy forwarding. | `response_optimizer` | Direct match, but current method is not a manifest-derived operation contract. |
| `ModelRouter` | Selects a provider/model route. | `model_router` | Direct match. Keep learned selection outside OCLA; define only bounded routing contract and policy proof. |
| `EfficiencyAnalyzer` | Calculates read efficiency/density. | `evaluation_provider` and planner input | Partial. It is a metric analysis primitive, not a common quality/compliance evaluator. |
| `ConfigTuner` | Proposes adaptive read-mode changes. | `context_planner` / evaluation support | Partial. A proposal is not a promoted pipeline decision. |
| `ExperimentRunner` | Runs routing evaluation. | `evaluation_provider` / `execution_provider` support | Partial. Split evaluation semantics from any bounded execution semantics before declaring a v0 type. |
| `ConnectorScheduler` | Schedules provider connector jobs. | `execution_provider` | Partial. No manifest-declared entrypoint, isolation, resource bound, or execution-mode contract. |
| `AgentGateway` | Relays and routes agent messages. | `execution_provider` support | Partial. It is an integration transport, not yet a bounded capability execution contract. |
| `DeliveryRegistry` | Reuses and records cross-agent content delivery. | `context_cache` | Partial and live. Define cache key scope, invalidation, retention/recovery, and isolation instead of elevating the implementation detail into a v0 type. |

### 5.1 Target categories without a first-class current trait

| Target v0 type | Existing substrate | Gap |
|---|---|---|
| `shell_output_optimizer` | `RtkShellAdapter` and shell-output code. | No legacy service trait; the adapter is not in the normal adapter registry. |
| `retrieval_provider` | Search, tree, and provider infrastructure. | No OCLA registration or typed candidate-supply contract. |
| `knowledge_provider` | Knowledge graph/store/router infrastructure. | No OCLA contract for provenance, authority, freshness, classification, and stable fact identity. |
| `memory_provider` | Session, CCP, agent, and memory stores. | No OCLA memory namespace, freshness, read/write permission, or lifecycle contract. |
| `policy_decision_provider` | Caller/adaptor constraints and configuration policies. | No first-class decision capability, scoped grant, denial record, or composable policy operation. |

## 6. CapabilityManifest v0 gap matrix

| Required v0 component | What exists now | Gap to close |
|---|---|---|
| Immutable identity | `CapabilityManifestV1` has ID and version; adapter key uses both. | Add canonical manifest content hash and artifact digest; reject an existing ID/version with different immutable content. |
| Publisher/package integrity | Provider string only. | Add publisher identity/key, license, artifact digest, canonical manifest signature, installation source, and verification state. |
| Exact type and operations | Generic `CapabilityKind`; free-form surfaces. | Add the 14-value v0 `CapabilityType` and declared operation IDs. Do not reuse the 15-value legacy enum. |
| Execution declaration | Independent `local` and `remote` booleans. | Require exactly one `in_process`, `local_process`, or `remote` mode plus transport/entrypoint, concurrency, timeout, and resource limits. |
| Input/output contract | Optional manifest schema references; three hard-coded invocation inputs. | Add operation-level media types, schema refs, byte/token limits, protected fields/spans, and typed/referenced output envelopes. |
| Fidelity and recovery | `Reversibility` and `Determinism` exist. | Add explicit lossiness and recoverability, bind claims to protected-span and recovery conformance. |
| Permission request and grant | Generic invocation allowlists/limits. | Add requested permissions, scoped grant, admission decision, and denial/fallback behavior; record the decision in the receipt. |
| Cache contract | No manifest cache semantics. | Add `cache_safe`, key scope, provider-prefix preservation, invalidation, and retention/recovery declarations. |
| Performance declaration | Per-call timeout and token/latency observations. | Add manifest latency budget, resource class, concurrency, output accounting, and cost declaration where relevant. |
| Trust/privacy | Data movement and classifications exist. | Add raw-content egress flag, endpoint allowlist, regional boundary, install trust state, and policy evaluation. |
| Telemetry/receipt linkage | Adapter observation and several independent evidence types. | Establish one public observation/failure owner and required invocation metrics, evidence references, and receipt linkage. |
| Local capability registry | In-memory test-populated `AdapterRegistry`. | Add real runtime population, enabled/disabled state, health, compatibility, conformance result, trust, and audited disablement. |
| Conformance | Basic protocol fixture suite. | Validate the v0 schema at admission and add base plus type-specific suites that gate required claims. |

### 6.1 Public type ownership conflict

There are two incompatible ownership paths for invocation records:

- Compiled `core::ocla::invocation` owns `CapabilityInvocation`, `CapabilityObservationV1`, `CapabilityFailureMode`, and `CapabilityResult`.
- Uncompiled `lean-ctx-ocla/src/{manifest.rs,observation.rs,failure.rs}` contains overlapping validator, observation, and failure records.

Before exporting any orphan module, choose one public owner. The recommended owner is the protocol crate for versioned wire records, with `lean-ctx-ocla` owning typed traits, manifest/registry interfaces, adapters, and conformance APIs. The main runtime should consume those records instead of defining a parallel public shape.

## 7. Path to CapabilityManifest v0

### Step 1 — Freeze compatibility boundaries

Create an explicit compatibility decision before changing behavior:

- Keep the published `CapabilityManifestV1` wire contract readable and writable.
- Introduce a separate, additive `CapabilityManifestV0` Rust record in `lean-ctx-protocol`; do not repurpose `extra` as an undocumented v0 contract.
- Provide an explicit, fallible compatibility projection only for V1 fields that can be represented without inventing a permission, digest, operation, or execution mode.
- Leave `OclaRegistry` and the existing trait call sites in place while the reference path is feature-flagged and compared.

### Step 2 — Define the smallest stable manifest

Put the serializable v0 contract and its validators in the protocol layer. The initial public types should include:

- `ResolvedCapabilityIdentity { id, version, manifest_digest, artifact_digest }`
- `CapabilityType` and `OperationSpec`
- `ExecutionSpec` for exactly one execution mode
- `ContentContract` for input/output media type, schema, size, and protected fields/spans
- `FidelitySpec`, `PermissionRequest`, `CacheSpec`, `PerformanceSpec`, `TrustPrivacySpec`, and `TelemetrySpec`
- `PolicyGrant` / `PolicyDecision` as invocation records, not mutable manifest fields

Specify canonical serialization before signatures or digest checks are implemented. Manifests must be immutable at a resolved identity; aliases may be used only for discovery, never in a Profile or Receipt.

### Step 3 — Make admission and conformance real

Replace the current test-only conformance use with a registration gate:

1. Parse and canonicalize the manifest.
2. Validate the complete v0 shape and invariants.
3. Verify artifact digest/signature when the installation policy requires it.
4. Check a compatible base and type-specific conformance result.
5. Evaluate a scoped local/enterprise policy grant.
6. Register the resolved identity only after all gates pass.

The base suite covers schema, identity, I/O contracts, permission denial, health, timeout/cancellation, output accounting, telemetry, and receipt linkage. The first type suite is `compression_provider`, covering lossiness, determinism, recovery, protected spans, and cache claims.

### Step 4 — Upgrade the operation envelope before routing traffic

Replace the three fixed `CapabilityInput` variants with operation-specific envelopes that carry:

- the resolved identity and operation ID;
- input references or bounded content plus content contract;
- task, policy-decision, classification, and receipt lineage;
- deadline/cancellation and allocated resource limits; and
- an output value/reference that can be consumed by the next pipeline step.

The current observation should become or convert to a protocol-owned record containing identity/digest, operation, size/token accounting, latency, status/error class, fallback, recovery reference, cost where relevant, policy decision, and receipt linkage.

### Step 5 — Promote `AdapterRegistry` into the local registry

Evolve rather than duplicate `AdapterRegistry`:

- Key entries by `ResolvedCapabilityIdentity`, not ID/version alone.
- Store adapter/endpoint, manifest, enabled state, health, installation source, trust verification, conformance evidence, and granted local policy.
- Register native in-process capabilities at runtime construction.
- Add deterministic `list`, exact `info`, `enable`, `disable`, `doctor`, and `conformance` operations.
- Make disable prevent new dispatch without changing historical Profile, Plan, or Receipt records.

Local-process and remote transports are later implementations of the same boundary. They must not be introduced as special registries or bypass admission.

### Step 6 — Normalize the native compression path

Build the first vertical slice around `BuiltinCompressionProvider`:

1. Author its v0 manifest with type `compression_provider`, operation `compress`, `in_process` execution, bounded source/result contracts, explicit lossiness and recovery behavior, local-only trust boundary, permissions, cache rules, latency budget, and telemetry.
2. Make its adapter return the actual normalized operation output or an approved content reference, rather than measurements alone.
3. Register it at normal runtime startup in the local registry.
4. Route the canonical `core/tool_lifecycle.rs` compression projection through the feature-flagged registry path.
5. Preserve existing invalid-input and fail-closed behavior; test old and new dispatch against identical fixtures.
6. Emit a receipt-linked observation using the resolved identity.

`NativeContextAdapter` is valuable evidence that a manifest-backed local optimization can enforce paths, token budgets, identity matching, timing, and output measurements. It should be converged with, not run alongside indefinitely with, `BuiltinCompressionProvider`.

### Step 7 — Pin identity through the proof loop

Add `ResolvedCapabilityIdentity` to the relevant Performance Profile, `ExecutionPlanV1`, `ExecutionReceiptV1`, and benchmark records. The same exact identity must survive:

```text
Profile selection -> Execution Plan -> registry lookup -> invocation observation
                  -> Benchmark result -> Execution Receipt
```

This is the Phase B proof that a manifest is more than discoverability metadata. It enables rollback, reproducibility, disablement checks, and trustworthy comparison.

### Step 8 — Expand by semantic category, not legacy trait order

After compression proves the path, normalize the direct matches (`response_optimizer`, `model_router`, `outcome_tracker`) one at a time. Then introduce missing types when a real native implementation needs them. Consolidate evidence and execution helper traits behind operations rather than preserving all 15 legacy service names as permanent public capability types.

## 8. Recommended implementation order and effort

Estimates are engineering days for a small focused sequence, including unit/contract tests and documentation. They exclude a broad workspace refactor, external partner integration, marketplace work, learned selection, and enterprise control-plane delivery.

| Order | Component | Deliverable and acceptance condition | Depends on | Effort |
|---:|---|---|---|---:|
| 0 | Compatibility decision and inventory correction | Adopt this mapping; identify V1/V0 ownership; correct stale 14-count status/docs in a separate small change. | WS-4 | 1–2 days |
| 1 | v0 protocol types and canonicalization | Add additive `CapabilityManifestV0`, resolved identity, schema fixtures, canonical bytes/digest rules, and explicit V1 compatibility boundary. | 0 | 4–6 days |
| 2 | Validator and base conformance | Full manifest validation plus base conformance fixtures that can reject invalid admission. | 1 | 4–6 days |
| 3 | Operation/policy/observation contract | Typed operation input/output, policy grant/decision, standardized failure and receipt-linked observation records. | 1 | 4–6 days |
| 4 | Local registry runtime | Runtime registration, exact resolved lookup, enable/disable, health, conformance and policy admission hooks. | 2–3 | 4–6 days |
| 5 | Native compression vertical slice | Manifest-backed `BuiltinCompressionProvider`, actual output hand-off, feature-flagged canonical caller, parity/failure tests. | 3–4 | 5–7 days |
| 6 | Profile/plan/receipt/benchmark pinning | Persist the same resolved identity across the vertical proof loop and benchmark it. | 5 | 3–5 days |
| 7 | Compression type conformance and operator diagnostics | Compression-specific tests plus local CLI list/info/doctor/conformance diagnostics. | 5–6 | 4–5 days |

**Critical-path estimate:** 29–43 engineering days. Some protocol, conformance, and envelope design can proceed in parallel after the compatibility decision, but the production switch must remain sequential: contract -> admission -> local registry -> compression -> proof-loop pinning.

## 9. Explicit non-goals and guardrails

- Do not make the current count of 15 services define the v0 taxonomy of 14 types.
- Do not register every existing function mechanically or remove the working legacy registry before the compression path proves parity.
- Do not expose `lean-ctx-ocla/src/manifest.rs`, `observation.rs`, or `failure.rs` merely by adding `pub mod`; resolve public ownership and dependencies first.
- Do not use `CapabilityManifestV1.extra` as the de facto v0 schema or silently reinterpret `local`/`remote` booleans as a single execution mode.
- Do not permit unsigned/untrusted or remote capabilities to bypass manifest, policy, health, conformance, and receipt gates for convenience.
- Do not add workload ranking, learned selection, fleet rollout, marketplace, or commercial performance intelligence to OCLA Phase B.
- Keep uncertain old/new routing feature-flagged and tested in both states; rollback by defaulting to the legacy path or reverting the isolated change.

## 10. Phase B exit criteria

The capability substrate is ready to move beyond the first slice only when all of the following are true:

1. `CapabilityManifestV0` is versioned, canonicalized, fixture-backed, and validated at registration.
2. A resolved identity pins ID, version, manifest digest, and artifact digest.
3. The normal runtime local registry admits and dispatches an enabled native capability under a scoped policy decision.
4. `BuiltinCompressionProvider` runs through that contract without regressing its existing fail-closed behavior.
5. The compression type suite proves its declared fidelity, recovery, determinism, cache, output-accounting, and telemetry claims.
6. One Performance Profile, Benchmark result, and Execution Receipt contain the same resolved identity and receipt-linked observation.
7. Disabling the capability prevents future dispatch while preserving historical evidence.

At that point LeanCTX has one credible, composable capability path. Additional target types and external local-process proof can build on the same contract instead of extending the legacy service registry.
