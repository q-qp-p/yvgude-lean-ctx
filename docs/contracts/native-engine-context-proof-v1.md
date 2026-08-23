# Native Engine Context Proof v1

> **Status:** internal Engine implementation proof. This is not an SDK facade,
> a hosted API, or a Cloud contract.

This document fixes the evidence boundary for the first local Engine Interface
v1 implementation: `capability://leanctx/context-optimization@1.0.0`.

## Scope

The implementation in `core::engine_interface` binds exactly one rooted local
context operation to the versioned records in `lean-ctx-protocol`:

1. Generic internal callers supply a bounded input identity and admission
   decision. The production `ctx_read` entry point accepts only the canonical
   path, raw authorized snapshot and actual gate admission; it derives every
   Engine identity internally.
2. A rejected decision produces `rejected` / `policy_rejected` and does not
   invoke the native adapter.
3. A standalone admitted decision reads and transforms the source once through
   the existing jailed native adapter. The production `ctx_read` bridge instead
   supplies the already authorized, bounded, no-follow worker snapshot, so the
   Engine cannot race a second disk read. The raw snapshot SHA-256 becomes the
   input reference; redaction occurs inside the request factory and the exact
   post-policy Engine-input digest is checked before success is emitted.
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
engine-interface/v1/recovery/<sha256>.json
```

- Directories are private on Unix. The deepest existing data-root ancestor is
  acquired directly by a no-follow final-component `open`; there is no separate
  metadata probe or canonicalize/reopen window. Missing root components and
  every later `engine-interface/v1/<class>` component are opened or created
  with `openat`/`mkdirat` relative to the held parent descriptor and no-follow
  protection. New
  content-addressed artifacts are written to an exclusive,
  no-follow temporary leaf through the held final-directory descriptor. Only a
  fully written, permission-hardened and synchronized temporary artifact may be
  published under its digest by descriptor-relative atomic no-replace
  publication (`renameat2` or contained `linkat`); an existing digest path is
  never overwritten. The named temporary entry's device/inode identity is
  checked against the held file before and after publication; a swapped entry
  is removed and rejected. Any failure before publication leaves the final
  digest path absent and retryable. Unix artifacts use mode `0600`.
- Windows opens only the drive/UNC anchor by absolute name, validates that
  handle's final DOS path, then walks every data-root and artifact component by
  held handles with native relative create/open operations and reparse
  protection; it has no probe/canonicalize/reopen window. Relative rename and
  disposition complete publication. If a required native
  primitive or runtime filesystem behavior is unsupported, the writer returns
  the stable `engine_artifact_boundary_unsupported` category and never falls
  back to pathname mutation. Any provisional directory remains confined beneath
  the held root; temporary leaves are delete-on-close and cleaned by handle.
  Artifact files are flushed before publication. Directory-handle flush is
  applied where the filesystem supports it; documented unsupported directory
  flush results neither weaken containment nor invalidate an already verified
  content-addressed artifact.
- Existing artifact paths must be regular, non-symlink files whose contents
  match their address.
- P1 rejects pre-existing symlink/reparse escape and never writes payload bytes
  outside the resolved data-root object. That object, not a later pathname
  lookup, is the boundary after binding. Deterministic root- and parent-swap
  sentinels relocate the held tree or output directory and replace the old
  pathname; publication remains inside the held object or fails closed, and the
  replacement/outside directory remains byte-for-byte unchanged.
- Receipt JSON is canonical and contains references, digests, status and
  measurements only. It intentionally excludes raw source and output bytes.
- The production source reference binds the canonical resolved path by SHA-256;
  the input reference binds the raw worker snapshot, while `input_digest` binds
  the redacted bytes actually consumed by the Engine.
- A rejected invocation that cannot safely resolve its source instead binds a
  SHA-256 identity of the requested absolute path under the distinct
  `source:requested-path-sha256` namespace; raw path bytes are never persisted.
- If receipt persistence fails, a separate canonical recovery artifact records
  the invocation and terminal observation without source or output payload.

## Production runtime path

An effective cold single-path `ctx_read` with `mode="aggressive"` and explicit
`engine_interface="v1"` invokes this Engine bridge after the context gate and
before the response is assembled. Omission preserves the exact legacy path,
including no Engine artifact side effects. The v1 boundary rejects implicit or
non-aggressive modes, every `paths` value, line windows, raw mode,
aggressiveness tuning and symbol protection; these inputs cannot silently alter
an invocation whose fixed shape is not represented in its identity. Its
admission reference is the SHA-256 identity of the actual gate decision, including the
requested/overridden mode and bounded policy signals. Budget blocks, pressure
downgrades, mode overrides and triage filtering produce a deterministic
`rejected` receipt without entering the native adapter. Existing warm-cache reads
remain untouched when Engine v1 is omitted. Explicit Engine v1 forces a fresh
bounded worker snapshot; identical calls reuse the same content-addressed
output and receipt identities rather than silently accepting a legacy cache hit.

On Unix, the production worker opens the source through the Engine root one
component at a time without following links. On Windows, it validates the
opened regular-file handle's normalized final path against the canonical root
before reading. The Engine receives that handle-backed raw snapshot and
canonical identity; it does not resolve or read the caller path a second time.
Engine construction fails closed when its root cannot be securely canonicalized;
it never substitutes the unresolved caller path. Materialized compression runs
in a dedicated worker behind a real
30-second host deadline. A timed-out worker cannot persist an artifact and late
completion cannot publish output. The receiving Engine records the terminal
`failed` / `resource_limit` observation and receipt itself.
Invocation identity includes Engine ID/version, capability ID/version in
addition to source, input, policy, mode and deadline identities.

The existing `ctx_read` renderer remains authoritative until measured parity:
successful response bytes do not include Engine metadata. A receipt-recording
failure is prefixed with the stable
`[ENGINE RECEIPT WARNING] code=engine_record_unavailable` marker while legacy
content stays available; a terminal Engine failure includes its receipt and
recovery reference in that warning. Receipt-write failures include their durable
recovery reference; a later cold/fresh read retries normal output and receipt
persistence.
Operational traces also record successful receipt references.

## Failure mapping

| Condition | Engine record | Recovery |
| --- | --- | --- |
| Pre-admission rejected | `rejected` / `policy_rejected` | none |
| Production source admission cannot complete safely | `rejected` / `policy_rejected` | requested-path hash only |
| Standalone native source cannot be read | `failed` / `source_unavailable` | supplied input reference |
| Read bytes differ from supplied identity | `failed` / `source_integrity_mismatch` | supplied input reference |
| Native input/output/timeout limit | `failed` / `resource_limit` | host decides |
| Native request shape invalid | `failed` / `internal` | host decides |
| Output artifact cannot persist | `failed` / `internal`, retryable | receipt plus input reference |
| Receipt cannot persist | visible caller warning | content-addressed recovery artifact |

The bridge maps typed native failures; it does not classify errors by parsing
human-readable strings.

## Verification

The proof covers admitted execution, canonical source and output integrity,
deterministic version-scoped receipt identity across repeated calls, real gate
admission,
single-snapshot production dispatch, raw/redacted identity separation, durable
and visible receipt-write failure plus successful retry, exact legacy omission
even when Engine artifact storage is unavailable, rooted-source refusal,
artifact-directory symlink containment, hard host deadline with no late output,
permission re-hardening, no adapter invocation on rejection,
structured source recovery, source-integrity mismatch, redaction non-disclosure,
tamper/symlink refusal and the versioned rejected-receipt golden fixture at
`engine-interface/v1/rejected-receipt.json`. Run:

```sh
cd rust
cargo test --lib core::engine_interface::tests
cargo test --lib tools::registered::ctx_read::engine::tests
cargo test --lib mcp_aggressive_read_
cargo test --lib engine_v1_rejects_
cargo test --lib engine_v1_rooted_read_failure_never_falls_back_to_outside_content
cargo test --lib omitted_engine_interface_preserves_legacy_image_and_binary_paths
cargo test --lib tools::ctx_read::file_io::tests
cargo test --lib tools::registered::ctx_read::image::tests
```

Volatile latency remains runtime telemetry and is deliberately absent from the
content-addressed Engine receipt; identical admitted inputs, policy decisions
and canonical sources produce the same invocation, output and receipt identity.
