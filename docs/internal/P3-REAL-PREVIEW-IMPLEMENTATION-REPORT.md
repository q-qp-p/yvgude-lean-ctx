# P3 Real Python Preview implementation report

Status: exit candidate complete
Date: 2026-08-24

## Immutable identities

- P2 accepted integration: PR `#1522`,
  `github/main@8ebf61a21c063a1d0a86be33511588d27d7ca71e`.
- P2 exact-SHA evidence: CI `32758619963`, Security Check `32758619955`,
  CodeQL `32758619966`; all succeeded.
- P3 baseline: `8ebf61a21c063a1d0a86be33511588d27d7ca71e`.
- P3 branch: `feat/p3-real-python-preview`.
- P3 implementation candidate:
  `87116a348dc85335354b2f30ab8703e931051962`.

## Transport and ownership decision

P3 adds a strict local process transport with exactly two explicit Engine
operations:

```text
lean-ctx engine context-view --project-root ROOT --json-file REQUEST
lean-ctx engine recover      --project-root ROOT --json-file REQUEST
```

The JSON wire pins schema `1`, transport `1`, and Engine interface `1.0.0`.
It returns the real public Engine invocation, observation, measurements,
receipt link, bounded view, and recovery descriptor. The full rationale is in
`execution/P3-ENGINE-TRANSPORT-DECISION.md`.

Python Preview owns Product task/session identity, state, explicit plan intent,
source/view relationships, host execution, completion/abort, explicit host
outcome, degradation policy, and SDK receipt projection. Apache Engine owns
rooted admission, security, bounded shaping, invocation, observation,
measurement, canonical artifacts, and exact recovery verification. Rust gained
no `/v1/sessions`, completion, abort, agent loop, Product Plan, Cloud, tenant,
Kit, Profile, or Workspace lifecycle.

## Real vertical slice

```text
customer Python host
→ LeanCTX.embed(task, project_root)
→ Python ContextSession + ContextSource + explicit ContextPlan
→ real local `lean-ctx engine context-view`
→ NativeContextEngine execution
→ canonical EngineInvocationV1 + EngineObservationV1 + receipt link
→ Python ContextView
→ host-owned model/agent execution
→ explicit completion or abort
→ truthful ContextReceipt
→ `ContextView.recover()`
→ real `lean-ctx engine recover`
→ exact digest-verified admitted source
```

No MCP `ctx_*` call, fake Runtime, server-side Product session, or Cloud is
required by this path.

## Changed runtime and source files

Rust Engine/runtime:

- `rust/src/core/engine_interface.rs`
- `rust/src/core/engine_interface/tests.rs`
- `rust/src/cli/engine_cmd.rs`
- `rust/src/cli/mod.rs`
- `rust/src/cli/dispatch/mod.rs`
- `rust/src/cli/dispatch/help.rs`

Python Preview:

- `packages/python-lean-ctx/lean_ctx/contracts.py`
- `packages/python-lean-ctx/lean_ctx/engine.py`
- `packages/python-lean-ctx/lean_ctx/core.py`
- `packages/python-lean-ctx/lean_ctx/session.py`
- `packages/python-lean-ctx/lean_ctx/receipt.py`
- `packages/python-lean-ctx/lean_ctx/errors.py`
- `packages/python-lean-ctx/lean_ctx/__init__.py`
- `packages/python-lean-ctx/pyproject.toml`
- `packages/python-lean-ctx/README.md`

Tests/fixture:

- `packages/python-lean-ctx/tests/test_engine.py`
- `packages/python-lean-ctx/tests/test_agents_sdk.py`
- `packages/python-lean-ctx/tests/fixtures/engine-interface-v1/compatibility.json`

## Framework and native-host evidence

The maintained framework path uses the actual `openai-agents==0.19.4`
`Agent` and `Runner.run_sync`. A provider-free deterministic custom `Model`
exercises the real framework lifecycle against the real Rust Engine. The
original `RunResult`, final output, and tool items are preserved. An agent/model
failure is re-raised as the identical exception object and attached to the
aborted receipt. No global patch or hidden interception exists. The supported
P3 path is synchronous; streaming is not claimed.

The native Embed test leaves the host call completely customer-owned:
prepare a real Engine view, execute arbitrary host work, call `complete`, then
recover the exact source. The host result object is retained by identity.

## Evidence and accepted-quality semantics

Engine delivery produces only factual execution evidence. P2 changed the
production Attach delivery receipt to `ReceiptOutcome::Unknown` and added
`successful_delivery_does_not_imply_task_acceptance`; accepted/rejected values
remain reachable only through explicit host/evaluator feedback paths.

P3 defaults every successful Engine-backed `ContextReceipt` to outcome
`unknown`. `complete(..., outcome=...)` is the explicit host boundary.
`ContextReceipt.verify()` verifies Engine evidence integrity only and never
interprets host quality. A fail-open bypass produces an unsealed receipt with
an explicit degradation and cannot fabricate Engine coverage or acceptance.

## Compatibility fixture

`engine-interface-v1/compatibility.json` pins:

- Python Preview `1.0.0`;
- Engine `3.9.20` and Engine interface `1.0.0`;
- wire/schema version `1`;
- request contract;
- invocation identity and policy admission;
- observation, measurements, receipt link, source/output digests;
- recovery reference;
- exact expected SDK projection.

`test_engine_v1_compatibility_fixture_projects_exact_sdk_contract` parses the
fixture through the production Python parser and compares its exact projection.

## Failure and degradation matrix

| Condition | `fail_open=True` | `fail_open=False` / invariant |
| --- | --- | --- |
| Engine unavailable | Host may continue; unsealed `engine_unavailable` receipt | Predictable unavailable error; aborted session |
| Deadline | Host may continue; unsealed `engine_timeout` receipt | Predictable timeout; aborted session |
| Policy rejection with factual view | Explicit degraded/rejected view; no acceptance | Rejected error |
| PathJail/unsafe request | Never fail-open | Protocol/security error |
| Malformed/unknown-version observation | Never fail-open | Protocol error; aborted session |
| Missing receipt link | Host may continue only as explicit unsealed degradation | No sealed evidence fabricated |
| Missing recovery source/artifact | No lossy view fallback | Recovery error |
| Changed digest/symlink/outside source | Never recovered | Engine rejects |
| Agent exception | Exact exception re-raised and retained | Aborted truthful receipt |
| Receipt sealing/verification failure | `verify() == false` | Never treated as accepted evidence |

## Clean-machine proof

Host Python `3.9` was rejected because the Preview and Agents SDK require
Python `>=3.10`. The clean proof used Python `3.11.14`:

```bash
/opt/homebrew/bin/python3.11 -m venv /private/tmp/leanctx-p3-venv311
/private/tmp/leanctx-p3-venv311/bin/python -m pip install --upgrade pip
/private/tmp/leanctx-p3-venv311/bin/python -m pip install \
  -e '/private/tmp/leanctx-p3-real-preview/packages/python-lean-ctx[openai-agents,test]'
LEAN_CTX_ENGINE_BINARY=/private/tmp/leanctx-p3-real-preview/rust/target/debug/lean-ctx \
LEAN_CTX_DATA_DIR=/private/tmp/leanctx-p3-clean-engine-data \
/private/tmp/leanctx-p3-venv311/bin/python -m pytest -q -rs \
  /private/tmp/leanctx-p3-real-preview/packages/python-lean-ctx/tests
```

Result: `122 passed, 2 skipped`. The skips are unrelated optional experimental
adapters (`langchain_core` and `litellm` absent). Both actual OpenAI Agents SDK
tests and the real Engine context/recovery tests ran. The documented README
scenario uses the same production path and produced a sealed factual receipt
plus exact source recovery.

Rollback:

```bash
/private/tmp/leanctx-p3-venv311/bin/python -m pip uninstall -y lean-ctx-python
/private/tmp/leanctx-p3-venv311/bin/python -c 'import agents; print(agents.__version__)'
/private/tmp/leanctx-p3-venv311/bin/python -c 'import lean_ctx'
```

The package uninstall succeeded; `agents` remained importable at `0.19.4`;
`lean_ctx` became unavailable. The host framework was not modified.

## Gates and OSS preservation

- `cargo fmt --check`: PASS.
- `cargo clippy --all-features -- -D warnings`: PASS.
- Engine interface tests: `29 passed`.
- `ctx_read` Engine bridge tests: `6 passed`.
- Aggressive Engine read tests: `2 passed`.
- Engine rejection tests: `3 passed`.
- Rooted-read failure/no-fallback: `1 passed`.
- Legacy image/binary preservation: `1 passed`.
- Rooted file I/O/PathJail tests: `3 passed`.
- Open-core boundary: PASS, five frozen protocol surfaces.
- OCLA contract/conformance verifier: PASS, all 18 cases.
- Security/protocol-freeze/OCLA Python suite: PASS.
- Python Preview suite in clean environment: `122 passed, 2 skipped`.
- `git diff --check`: PASS.
- Independent `gpt-5.6-luna`/max re-review of candidate
  `87116a348dc85335354b2f30ab8703e931051962`: PASS, no blocking findings.

The unfiltered Rust library suite result is `10408 passed, 1 failed, 22
ignored`. The sole failure is the previously established baseline/environment
case
`tools::ctx_shell::tests::validate_blocks_redirects_and_piped_tee_into_project_root`.
P2 reproduced identical behavior on clean current default-branch and candidate
checkouts; no ignore or weakening was added. Environment: macOS `26.2`
(`25C56`), arm64, Rust/Cargo `1.97.1`.

## Live provider and limitations

`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, and `GEMINI_API_KEY`
were absent.

```text
LIVE PROVIDER SMOKE: UNVERIFIED
```

No live-provider, streaming, generalized multi-source planning, automatic
selection, Cloud, or Production SDK claim is made.

## Final exit assessment

- P2 is integrated and green on exact `github/main` SHA.
- The real Engine is the primary P3 execution and recovery proof.
- Python owns all Product session lifecycle semantics.
- Native Embed and actual OpenAI Agents SDK paths pass.
- Outputs, exceptions, evidence, degradation, and quality semantics are
  truthful.
- OSS coding-agent workflows remain available and covered by preservation
  tests.
- No blocker remains for P3 candidate integration.
- P4, SDK repository extraction, Engine decommission, Workspace, and Cloud did
  not begin.

Architectural answers:

1. Product Session lifecycle: Python Preview / future BSL Production SDK.
2. Engine ownership: explicit secure context mechanisms plus factual
   invocation, observation, measurement, and recovery.
3. Can users still use LeanCTX OSS for coding agents? Yes.
4. Did P3 make the BSL SDK unnecessary? No.
5. Can the future SDK depend only on public Engine contracts? Yes.
