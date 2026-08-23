# Invocation Context Binding v1

Status: normative local admission sidecar
Wire owner: `lean-ctx-protocol::InvocationContextBindingV1`
Media type: `application/vnd.leanctx.invocation-context-binding+json`

`InvocationContextBindingV1` is a signed, digest-only admission sidecar for
one Engine invocation. It binds the complete session, task, plan, invocation,
admission-policy, source, and capability context without embedding payload
bytes. The `invocation-evidence-manifest` uses the canonical digest of this
sidecar for its `invocation_admission` binding; the sidecar's `policy_digest`
then resolves the signed admission policy bytes.

## Wire shape

The JSON Schema in `invocation-context-binding-v1.schema.json` is normative.
Unknown fields are rejected recursively. Every digest is a canonical
`sha256:` plus 64 lowercase hexadecimal characters. Shared IDs and references
retain their protocol UTF-8 byte bounds and control-character rules.

The `decision` is always `admitted`; rejected decisions are not represented by
this sidecar. `source_bindings` contains exactly one `input` and any remaining
`context` entries. `capability_bindings` contains at least one selected
capability. The current V1 Engine operation is checked by a later adapter; this
sidecar does not prescribe a fixed operation count.

Source bindings are strictly ordered by `source_ref`. Capability bindings are
strictly ordered by `(capability_id, capability_version)`. This makes the
arrays deterministic while retaining the exact binding types used by
`InvocationEvidenceManifestV1`.

`not_before <= issued_at < expires_at` is a structural ordering invariant.
No current-time check, one-time consumption, revocation, or trust-store
decision is made by this protocol decoder; those are runtime and
cross-artifact gates.

## Signatures and canonical bytes

Canonical JSON means:

1. input is valid UTF-8 with no duplicate object key at any depth;
2. values contain no floating-point JSON numbers;
3. objects are recursively sorted by Unicode key and emitted compactly;
4. arrays retain their declared deterministic order; and
5. exact raw bytes, including key order, whitespace, escapes, and UTF-8, are
   the accepted representation.

`InvocationContextBindingV1::from_canonical_bytes` enforces all five rules,
rejects unknown fields, validates bounds and joins, and requires the exact
canonical bytes on re-encoding.

The signature covers:

```text
leanctx/invocation-context-binding/v1\0
<canonical JSON object with signature omitted>
```

`signature` is canonical Base64 for exactly 64 Ed25519 signature bytes (88
characters including `==`, with zero pad bits). `signer.algorithm` is exactly
`ed25519`; `signer.key_id` identifies an external key and
`signer.public_key_digest` identifies its 32-byte SHA-256 digest. The protocol
crate exposes canonical and signing-byte helpers; cryptographic key lookup and
signature verification remain the responsibility of the caller's trust and
runtime layer.

The complete canonical bytes, including `signature`, are the sidecar digest.
No self digest or circular reference field is present in this contract.

Cross-language byte vectors and adversarial inputs live under
`invocation-context-binding/v1/`. Implementations compare canonical bytes,
not only parsed object equality.
