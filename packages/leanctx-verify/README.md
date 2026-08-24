# leanctx-verify

Standalone offline verifier for LeanCTX evidence bundles. It has two deliberately
separate contracts: frozen audit archives (`evidence-bundle-v1`) and customer-proof
documents (`customer-proof-v2`). For auditors: **no LeanCTX installation or network
access is required.**

## Customer-proof V2

V2 verifies a canonical JSON document and its bounded, local artifact directory:

```
leanctx-verify v2 <customer-proof.json> \
  --trust-store <customer-trust.json> \
  --artifact-root <proof-directory> [--json]
```

V2 always requires an external canonical trust store. It never accepts an embedded
or self-attested key as proof of signer trust. A valid result covers canonical bytes,
strict JSON (including duplicate-key rejection), signed identity, artifact hashes and
path containment, matching/quality/replay joins, and the declared claim semantics.
See `docs/contracts/evidence-bundle-v2.md` and
`docs/contracts/evidence-bundle-v2-verification-v1.md` for the normative contract.

## Audit bundle V1 (frozen)

V1 remains an archive-integrity verifier:

```
leanctx-verify <bundle.zip> [--pubkey <hex ed25519 key>] [--json]
```

Five independent checks, each reported PASS/FAIL:

1. archive + manifest well-formed
2. every file matches its SHA-256 in the manifest (no additions/removals)
3. audit hash chain replays from anchor to head (no edit/insert/delete/reorder)
4. manifest Ed25519 signature verifies
5. per-entry signatures verify

Exit code `0` = VALID, `1` = INVALID, `2` = usage error.

Without `--pubkey` the manifest's embedded key is used (self-attested
mode — proves internal consistency only; never use it for V2). Auditors should obtain the
organisation's public key out-of-band; see
`docs/enterprise/reading-evidence.md` for the full auditor guide.

## Design constraints

* **Independent implementation.** This crate shares no code with the
  LeanCTX engine; it implements the published contract
  (`docs/contracts/evidence-bundle-v1.md`, OCP Part 4). A PASS therefore
  attests the *specification*, not "two copies of the same code agree".
* **Minimal dependencies** (`ed25519-dalek`, `sha2`, `serde_json`, `zip`),
  release binary statically stripped.
* **Mutation-tested.** CI flips single bytes in every payload region,
  truncates the chain and swaps keys — each must produce INVALID
  (`tests/verify_bundle.rs`).

## Build

```
cargo build --release   # → target/release/leanctx-verify
```

## Provider-free reference fixture

`fixtures/provider-free-v2/` is a committed, metadata-only customer-proof V2
fixture with all 13 referenced artifacts and an out-of-band test trust store.
It requires no provider, account, network access, LeanCTX daemon, or Engine
installation:

```
cargo run --release -- v2 \
  fixtures/provider-free-v2/customer-proof.json \
  --trust-store fixtures/provider-free-v2/trust-store.json \
  --artifact-root fixtures/provider-free-v2 \
  --json
```

The fixture signer is deterministic test material and is never a production
trust anchor. `tests/provider_free_fixture.rs` freezes the exact file set,
requires every verification step to pass, and proves that a copied fixture
fails after a single referenced artifact is changed.
