# Native Engine Context Proof v1

> **Status:** internal Engine implementation proof. This is not an SDK facade,
> a hosted API, or a Cloud contract.

This document fixes the evidence boundary for the first local Engine Interface
v1 implementation: `capability://leanctx/context-optimization@1.0.0`.

## Scope

The implementation in `core::engine_interface` binds exactly one rooted local
context operation to the versioned records in `lean-ctx-protocol`:

1. The caller supplies a bounded input reference, its expected SHA-256 digest,
   source references and an admission decision.
2. A rejected decision produces `rejected` / `policy_rejected` and does not
   invoke the native adapter.
3. An admitted decision reads and transforms the source once through the
   existing jailed native adapter. The exact input digest is checked before a
   successful observation is emitted.
4. The derived bytes are written locally under their SHA-256 identity. The
   observation contains only `output:<digest>` and measurements, never payload
   bytes.
5. A canonical receipt contains the invocation and the observation without a
   receipt link. Its SHA-256 is then attached as `EngineReceiptLinkV1`, avoiding
   a self-referential receipt digest.

## Local artifacts and containment

Artifacts live under the resolved local data directory:

```text
engine-interface/v1/outputs/<sha256>.txt
engine-interface/v1/receipts/<sha256>.json
```

- Directories are private on Unix; artifacts are atomically written with mode
  `0600`.
- Existing artifact paths must be regular, non-symlink files whose contents
  match their address.
- Receipt JSON is canonical and contains references, digests, status and
  measurements only. It intentionally excludes raw source and output bytes.

## Failure mapping

| Condition | Engine record | Recovery |
| --- | --- | --- |
| Pre-admission rejected | `rejected` / `policy_rejected` | none |
| Jailed source cannot be read | `failed` / `source_unavailable` | supplied input reference |
| Read bytes differ from supplied identity | `failed` / `source_integrity_mismatch` | supplied input reference |
| Native input/output/timeout limit | `failed` / `resource_limit` | host decides |
| Native request shape invalid | `failed` / `internal` | host decides |

The bridge maps typed native failures; it does not classify errors by parsing
human-readable strings.

## Verification

The unit proof covers admitted execution, source and output integrity,
deterministic identity/output across repeated calls, no adapter invocation on
rejection, structured source recovery and source-integrity mismatch. Run:

```sh
cd rust
cargo test --lib core::engine_interface::tests
```

The measured latency may differ between executions; identity, source lineage,
input digest and output digest must not.
