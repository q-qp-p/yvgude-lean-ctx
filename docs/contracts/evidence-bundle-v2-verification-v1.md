# Customer-Proof Evidence Bundle v2 Verification v1

Status: normative verification companion for
`evidence-bundle-v2.schema.json`. It adds no registry, hosted service, or
selection mechanism.

## Verification input

`leanctx-verify v2` accepts a canonical V2 JSON document, a caller-supplied
trust-store JSON document, and an artifact root directory. The V2 document is
not an archive and never supplies a public key. A structural-only parse is not
a proof and cannot make a supported claim eligible.

The artifact root is a local directory. Every `inventory.items[*].path` with
`availability: present` resolves below that root as a regular file. The
verifier rejects absolute, traversal, duplicate, escaping-symlink, missing,
oversized, or digest-mismatched files. `omitted` and `unavailable` entries may
be recorded, but cannot be a basis for a `supported` claim.

An inventory item with `kind: arm_receipt` is not opaque. Its exact bytes must
be a canonical, signed `ReceiptDocumentV1` with `schema_version: 1`. Each arm
must reference exactly one distinct receipt. Every task, plan, invocation,
identity, policy, and evidence digest named by that receipt must resolve to one
present inventory item. Task-envelope, execution-plan, engine-invocation, and
accepted-outcome sidecars are canonical JSON whose payload IDs, task join,
admitted capability/version, policy admission, input/source binding, and outcome
state agree exactly with the signed receipt; matching digest/kind alone is
insufficient. The verifier recomputes the receipt ID, verifies its
canonical padded-Base64 Ed25519 signature under the external trust store, and
enforces the Task → Plan → Invocation → Receipt → Outcome joins from
`receipt-document-v1.md` before any claim is eligible.

## Canonical signature and key identity

The verifier removes exactly `bundle_id`, `bundle_digest`,
`signing.signed_digest`, and `signing.signature`, then serializes the remaining
JSON recursively sorted, compact UTF-8. These unsigned canonical bytes are:

1. SHA-256 hashed to form `bundle_digest`.
2. Signed directly with Ed25519 to form `signing.signature` (standard padded
   Base64).

The signed digest string itself is not the Ed25519 message. For a raw 32-byte
Ed25519 public key `K`, `key_id` is `id:sha256:<SHA-256(K)>` and
`trusted_signer_ref` is `signer:<key_id>`. The verifier computes both values;
it does not accept a caller-declared alternate mapping.

## External trust store

The trust store is supplied independently of the bundle and is strict JSON:

```json
{
  "schema_version": "leanctx.customer-proof-trust-store/v1",
  "trust_revision": 1,
  "evaluated_at": "2026-08-24T00:00:00Z",
  "trusted_signers": [
    {
      "trusted_signer_ref": "signer:id:sha256:<public-key-digest>",
      "key_id": "id:sha256:<public-key-digest>",
      "public_key": "<64 lowercase hexadecimal characters>",
      "allowed_trust_bases": ["customer_configured", "out_of_band"],
      "receipt_key_ids": ["customer-receipt-key-2026"],
      "revision": 1,
      "admitted_at": "2026-01-01T00:00:00Z",
      "expires_at": "2027-01-01T00:00:00Z",
      "revoked_at": null
    }
  ],
  "receipt_chain_heads": [
    {
      "chain_id": "control-run-2026-08-22",
      "sequence_number": 14,
      "receipt_id": "sha256:<canonical-receipt-identity>"
    }
  ]
}
```

`receipt_key_ids` are bounded external aliases admitted for signed receipt
documents; they never contain key bytes. There must be exactly one matching
signer. The verifier evaluates key admission,
expiry, and revocation against the explicit `evaluated_at` snapshot and requires
the signer revision not to exceed `trust_revision`. Bundle and receipt issuance
must fall within the admitted key interval. Every arm receipt must equal the
externally supplied head for its chain; stale sequence numbers, alternate heads,
duplicate positions, and forks fail closed. A non-genesis head additionally
requires every predecessor as a present `receipt_predecessor` inventory item.
The verifier walks to sequence one, requires same-chain sequence-minus-one links,
and checks each `previous_signature_digest` against the SHA-256 digest of the
predecessor's decoded 64-byte Ed25519 signature. Unreferenced predecessors fail
closed. `local_identity` is never a customer-proof trust basis. Absent, stale,
expired, revoked, malformed,
duplicate, mismatched, or bundle-supplied trust information fails closed.

## Proof eligibility

A V2 proof is eligible only if canonical structure, digest/ID, inventory,
receipt/source joins, semantic joins, external signer trust, and both receipt and
bundle Ed25519 verification pass.
In particular, the verifier rejects unmatched arms, dangling refs, non-observed
cost claims, false quality status, partial bundles with broad supported claims,
and any supported claim whose basis is not a present verified artifact.
