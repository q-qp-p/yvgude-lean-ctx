# Invocation Evidence Manifest v1

Status: normative local evidence contract
Wire owner: `lean-ctx-protocol::InvocationEvidenceManifestV1`
Media type: `application/vnd.leanctx.invocation-evidence-manifest+json`

`InvocationEvidenceManifestV1` is the strict digest-only join for one admitted
Engine invocation. It does not replace `ReceiptDocumentV1`, add a ledger field,
or embed source, policy, capability, invocation, or receipt payload bytes.
Existing manifests remain readable; exact-proof admission additionally requires
the signed `InvocationContextBindingV1` sidecar described below.

## Wire shape

The JSON Schema in `invocation-evidence-manifest-v1.schema.json` is normative.
Unknown fields are rejected recursively. Collections contain at most 64
entries; source and capability collections contain at least one entry. All
digests use `sha256:` followed by exactly 64 lowercase hexadecimal characters.
Opaque identifiers and references use the shared bounded protocol primitives.
Schema `maxLength` is a Unicode code-point prefilter; an independent verifier
MUST additionally enforce the exact shared UTF-8 byte bounds (256 bytes for
opaque IDs and 1,024 bytes for protocol references). Generic JSON Schema alone
is insufficient: exact conformance requires three mandatory stages: schema
validation, semantic validation by
`InvocationEvidenceManifestV1::from_canonical_bytes`, and
`cross_artifact_join`. The machine-readable `x-conformance` metadata declares
the three stages and that `x-maxUtf8Bytes` is only a schema annotation whose
semantic check is mandatory. References that are whitespace-only or contain
C0/C1 controls, including U+0085, or contain U+FEFF are invalid; bounded
capability IDs containing U+FEFF are also invalid.

The `cross_artifact_join` stage belongs to the adapter/verifier, not this
protocol decoder. It MUST require policy roles iff the corresponding refs are
present in TaskEnvelope/Plan/Invocation, and resolve exact invocation, source,
policy, capability-manifest, Engine-receipt, and other referenced artifact
bytes and verify each digest. The decoder alone cannot satisfy this stage;
executable adversarial join cases land with adapter integration.

The `invocation_admission` exception is mandatory: its binding digest resolves
the signed canonical `InvocationContextBindingV1` bytes, and that binding's
embedded `policy_digest` resolves the exact signed admission-policy bytes.
`policy_ref` remains the invocation's policy locator; the binding digest is not
itself policy content and must not be treated as a policy byte digest.

`invocation_ref` is the canonical digest of the exact Engine invocation record.
`engine_receipt.receipt_digest` identifies the complete Engine receipt bytes,
and `engine_receipt.receipt_ref` is required to be exactly
`receipt:<receipt_digest>` (for example,
`receipt:sha256:aaaaaaaa…`). A matching digest with another locator is invalid.

`source_bindings` is the complete source lineage. Every source locator has one
digest binding, source locators and digests are unique, and exactly one binding
has role `input`; all other bindings have role `context`.

`policy_bindings` contains 1..=4 bindings and MUST contain exactly one
`invocation_admission` role. `task_region`, `task_model`, and `plan_decision`
roles are optional and each may occur at most once. The same exact
`policy_ref` + digest may alias across distinct optional roles (for example,
`task_model` and `plan_decision`), but `invocation_admission` must be unique
from every optional role. Each policy reference maps to one digest and each
digest maps to one policy reference; conflicting aliases, admission aliases,
duplicate roles, and duplicate identical bindings are invalid. A later
cross-artifact verifier compares the present roles with refs actually present
in the TaskEnvelope, Plan, and Invocation; producers MUST NOT fabricate
omitted policy bindings to satisfy a fixed role set.

`capability_bindings` maps each selected capability ID and SemVer pair to the
digest of its canonical `CapabilityManifestV1` bytes. Capability ID/version
pairs and manifest digests are unique.

## Canonical bytes

Canonical JSON means:

1. input is valid UTF-8 with no duplicate object key at any depth;
2. values contain no non-finite numbers, floating-point values, or integers
   outside the JSON-safe integer range;
3. objects are recursively sorted by Unicode key and emitted compactly;
4. arrays retain declared order; strings use JSON UTF-8 escaping rules;
5. exact raw bytes, including key order, whitespace, escapes, and UTF-8, are
   the accepted representation.

Schema `type: integer` may treat JSON `1.0` as numerically equivalent to `1`
at stage one. The canonical semantic stage rejects all floating-point JSON
numbers, so a `schema_version` encoded as `1.0` is invalid.

`InvocationEvidenceManifestV1::from_canonical_bytes` rejects duplicate keys,
trailing data, alternate whitespace, alternate key order, alternate string
escaping, unknown fields, and every invalid semantic binding before accepting
the manifest.

Cross-language byte vectors live under
`invocation-evidence-manifest/v1/`. Implementations compare canonical bytes,
not only parsed object equality.
