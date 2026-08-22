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
  "trusted_signers": [
    {
      "trusted_signer_ref": "signer:id:sha256:<public-key-digest>",
      "key_id": "id:sha256:<public-key-digest>",
      "public_key": "<64 lowercase hexadecimal characters>",
      "allowed_trust_bases": ["customer_configured", "out_of_band"]
    }
  ]
}
```

There must be exactly one matching signer. `local_identity` is never a
customer-proof trust basis. Absent, malformed, duplicate, mismatched, or
bundle-supplied trust information fails closed.

## Proof eligibility

A V2 proof is eligible only if canonical structure, digest/ID, inventory,
semantic joins, external signer trust, and Ed25519 verification all pass.
In particular, the verifier rejects unmatched arms, dangling refs, non-observed
cost claims, false quality status, partial bundles with broad supported claims,
and any supported claim whose basis is not a present verified artifact.
