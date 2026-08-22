# Customer-Proof Evidence Bundle v2

Status: additive customer-proof contract. The producer and independent verifier
are defined by the companion verification contract; no registry is introduced.

The canonical wire identifier is
leanctx.customer-proof-evidence-bundle/v2. The JSON Schema is
evidence-bundle-v2.schema.json. The golden fixtures are under
tests/fixtures/evidence-bundle-v2/.

## Boundary with audit evidence-bundle-v1

evidence-bundle-v1 is frozen audit evidence: a deterministic ZIP with
manifest.json, an audit/trail.jsonl chain, resolved policies, and coverage.
Its proof is archive integrity, audit-chain continuity, and manifest signing.

This v2 contract is a separate customer-proof observation for one matched
control/treatment workload. It is a JSON contract, not the v1 ZIP layout; it
does not contain or redefine an audit chain, policy pack, coverage report,
period manifest, or v1 verifier behavior. A v1 archive may appear only as a
bounded, explicitly typed inventory item
(frozen_audit_bundle_v1). A v2 document MUST NOT be presented as a v1 audit
bundle, and a v1 verifier MUST NOT be used as a v2 verifier.

This change is additive and contract-only. Existing Rust/Python producers,
claim code, and the v1 contract remain untouched. No registry is introduced:
trusted_signer_ref is resolved by the customer's pre-existing trust
configuration or an out-of-band reference.

## Canonical representation and identifiers

All digests are over bytes, not parsed values.

1. Serialize JSON as UTF-8 with recursively lexicographically sorted object
   keys, compact separators, no insignificant whitespace, and no trailing
   newline.
2. Preserve array order. The schema admits integers only where a number is
   used; emit no floating-point number, NaN, Infinity, or negative zero.
3. Escape strings using ordinary JSON escaping. The serialized bytes are the
   canonical input to SHA-256.
4. A digest is sha256:<64 lowercase hexadecimal characters>.
5. A content identifier is id:sha256:<64 lowercase hexadecimal characters>;
   it identifies the SHA-256 digest of the canonical object or bytes named by
   that identifier. A source revision is explicitly tagged
   git:<40 or 64 lowercase hexadecimal characters>.
6. bundle_digest is SHA-256 of the canonical document after removing
   bundle_id, bundle_digest, signing.signed_digest, and signing.signature.
   bundle_id is id: followed by the same digest.
7. signing.signed_digest MUST equal bundle_digest. Ed25519 signs the
   canonical bytes from step 6. The signature is standard padded Base64; the
   trusted key is not inferred from the signature.

The schema constrains representation. Recomputing digests, checking the
signature, and resolving trust are verifier obligations outside this
contract-only foundation.

## Matched arms and control identity

matched_arms.control and matched_arms.treatment are the two observations
being compared. Both identities MUST equal matched_arms.shared_identity for
every field named by match_basis; the schema cannot express cross-object
equality, so a verifier MUST enforce it.

The initial match basis is exactly provider, model, source revision, and
workload digest. The control arm is the unmodified comparison path; the
treatment arm is the path under evaluation. Different endpoints or proxy
routing may be recorded in an arm's identity, but MUST NOT change the shared
match identity. Arm IDs and artifact references are content identifiers, not
caller-chosen display labels.

Each measurement has an explicit status. observed means directly recorded,
estimated means derived by a documented method, unavailable means no value
was obtained, and not_applicable means the measurement is outside scope. A
zero integer with unavailable is not evidence of zero.

## Currency and quality

Currency uses an uppercase ISO-4217 code and integer micro-units only:
amount_micros is never a decimal or floating-point amount. Its status carries
the provenance of the amount. Cost comparisons MUST use the same currency and
MUST retain the two arm statuses.

Quality scores are integer milli-scores in the closed interval 0..1000.
quality.status is:

- preserved: the treatment score meets the declared comparison rule;
- degraded: the treatment score is below the control score;
- inconclusive: available observations cannot decide;
- not_measured: no quality comparison was run.

quality.confidence describes measurement confidence, not legal or commercial
certification. A quality claim is limited to this matched workload unless
its claim scope says otherwise; the contract does not establish
generalization.

## Replay and limitations

replay.status describes what a reviewer can reproduce:

- replayable: listed inputs and environment are sufficient for a deterministic
  replay;
- partial: only the listed subset can be replayed;
- not_replayable: replay requires unavailable inputs or services;
- not_attempted: no replay was run.

input_refs and result_refs MUST point to inventory ref values. Replay status
never upgrades claim validity by itself. limitations records explicit
unproven scope, including omission before capture, third-party attestation,
generalization beyond the workload, and production SLA. An empty limitation
list is allowed only when the document's unproven list still states that no
additional limitation was recorded; it does not mean the v1 audit guarantees
apply.

## Redaction and bounded inventory

redaction.class and every inventory item's redaction_class are normative:

- none: no redaction was applied;
- pseudonymized: stable identifiers replace direct identifiers;
- metadata_only: content is excluded and only metadata remains;
- content_removed: content was intentionally removed;
- secret_removed: secret material was removed;
- aggregated: individual observations were combined.

Redaction is not evidence of absence. The inventory is bounded to 128 items,
8 MiB per item, and 64 MiB total. Each item has a relative path, content
digest, byte size, availability, and redaction class. Inventory bounds are
verified before dereferencing any item.

## Claim validity

Each claim carries claim_validity:

- supported: the listed inventory evidence satisfies the claim's declared
  matched-run scope and the verifier's checks;
- inconclusive: evidence is present but cannot decide the claim;
- unsupported: the evidence does not support the claim;
- not_asserted: the bundle deliberately makes no claim.

supported is scoped evidence, not a guarantee of future outcomes, a
third-party attestation, or a certification. Claims MUST reference the
inventory items that justify them. A bundle with status: partial MUST NOT
contain a supported claim with scope: customer_workload or scope: general.

## Fixture and validation policy

The valid fixture is a canonical schema vector with matched arms, integer
currency micros, quality/replay/limitation semantics, redaction, bounded
inventory, claims, and a trusted signer reference. Invalid fixtures are also
canonical JSON and exercise strict unknown-field and non-integer-currency
rejection. The contract test validates the schema, canonical byte form, and
both valid/invalid fixture outcomes. It is a structural fixture, not a
trust-rooted proof-valid vector.

## Verification companion

The separate producer/verification boundary, canonical unsigned bytes, external
trust-store format, artifact-root checks, and proof-eligibility semantics are
normative in [evidence-bundle-v2-verification-v1.md](evidence-bundle-v2-verification-v1.md).
