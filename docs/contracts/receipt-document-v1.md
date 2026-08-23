# Receipt Document v1

Status: normative local evidence contract
Wire owner: `lean-ctx-protocol::ReceiptDocumentV1`
Media type: `application/vnd.leanctx.receipt+json`
Compatibility: additive; legacy `ExecutionReceiptV1` remains readable

`ReceiptDocumentV1` is the single signed truth for one local
Task → Plan → Invocation → Receipt → Outcome lineage.  Session receipts,
builders, ledgers, bundles, and reports may project it, but they must not
invent a second identity or rewrite accepted bytes.

The document contains bounded metadata and digest-only references.  Prompts,
completion text, source files, credentials, request headers, customer data,
and other private payloads are excluded and must remain in separately secured
evidence artifacts.

## Wire shape

The JSON Schema in `receipt-document-v1.schema.json` is normative.  Unknown
fields are rejected.  All collection limits are 64 entries.  Every digest is
`sha256:` followed by exactly 64 lowercase hexadecimal characters.  Names and
units use lowercase ASCII `[a-z0-9_]` and are at most 64 bytes.

Every bounded opaque identifier (including task, plan, invocation, chain,
capability, and outcome identifiers) is additionally limited to 256 UTF-8
bytes at runtime.  JSON Schema `maxLength: 256` is only a structural
code-point prefilter; because JSON Schema length is not a UTF-8 byte ceiling,
an independent verifier MUST enforce the 256-byte limit after decoding.

`lineage` binds typed task and plan IDs plus canonical task/plan digests,
bounded invocation ID plus canonical invocation digest, caller/agent identity
digest, one or more policy decision digests, and one or more capability ID /
SemVer / invocation-digest links.  Each capability invocation digest must equal
the lineage invocation digest; the separately persisted artifacts must resolve
the exact referenced bytes, not merely matching display fields.

`chain.sequence_number` starts at one.  Genesis omits both predecessor fields;
every later record includes `previous_receipt_id` and
`previous_signature_digest`.  A receipt cannot reference itself.  Writers
persist canonical receipt bytes before appending a chain link.  Retry is
idempotent by `receipt_id`; changing, deleting, inserting, or reordering an
accepted receipt invalidates the next link.

`previous_signature_digest` is the `sha256:` digest of the exact predecessor
receipt's canonical signature-coverage bytes (the bytes returned by
`ReceiptDocumentV1::signing_bytes`, with only its `signature` field omitted),
not a digest of the decoded signature text or of the full predecessor JSON.

## Canonical bytes and signatures

Canonical JSON means:

1. input is valid UTF-8 with no duplicate object key at any depth;
2. values contain no non-finite numbers, floating-point values, or integers
   above `9007199254740991` (`2^53 - 1`);
3. objects are recursively sorted by Unicode key and emitted compactly;
4. arrays retain their declared order; strings use JSON UTF-8 escaping rules;
5. the exact raw bytes, including key order, whitespace, escapes, and UTF-8,
   are the signed representation.

`receipt_id` is `sha256:` plus the SHA-256 of canonical document bytes with
`receipt_id` and `signature` omitted.  The Ed25519 signature covers canonical
bytes with only `signature` omitted, including the derived `receipt_id`, full
lineage, chain, evidence, outcome, timestamp, and signer metadata.

`signer.algorithm` is exactly `ed25519`.  `signer.key_admission` is exactly
`external_trust_store`; `signer.key_id` is a bounded external identifier and
never contains a public key or trust root.  The verifier supplies the trusted
key for that ID.  `signature` is standard RFC 4648 base64 for exactly 64
Ed25519 bytes, with canonical zero padding and exactly `==` padding.  A
syntactically valid signature is not cryptographic proof until a verifier
checks it against an externally admitted key.

The Rust decoder `from_canonical_bytes` rejects invalid UTF-8, duplicate keys,
trailing data, noncanonical whitespace, alternate key order, and alternate
escaping before typed validation.  Protocol structure, cryptographic
verification, trust admission, bundle inventory, freshness, arithmetic
replay, and cross-document joins are separate verifier obligations.

Cross-language byte vectors are checked in `receipt-document/v1/`: the full
document, `canonical-identity.json` (the receipt-ID input), and
`canonical-signing.json` (the Ed25519 input).  Implementations must compare
raw bytes, not merely parsed object equality.

## Values and provenance

Every value has a unique lowercase name and unit, an optional non-negative
safe integer, and digest-only evidence references.  Each digest occurs at most
once in `evidence_refs`; every formula, price-table, reconciliation, outcome,
and acceptance reference must resolve to exactly one listed digest.  Direct
value evidence cannot repeat a formula, price, or invoice digest.

| Classification | Required | Forbidden |
| --- | --- | --- |
| `measured` | safe integer and measurement evidence | formula, price, reconciliation |
| `estimated` | safe integer and assumption evidence | formula, price, reconciliation |
| `calculated` | integer, measurement/assumption evidence, formula and price-table evidence | reconciliation |
| `reconciled` | calculated provenance plus an invoice evidence digest in `reconciliation_digest` | missing or non-invoice reconciliation evidence |
| `unavailable` | no value and no provenance | all evidence and calculation claims |

Evidence references use privacy-safe locators only: `artifact://`,
`evidence://`, `bundle://`, `source://`, or `urn:` with no query, fragment,
userinfo, local file path, control character, whitespace, or traversal.  Media
types are bounded ASCII RFC token `type/subtype` values.  Evidence payload
bytes stay outside the receipt and may carry their own signature status.

## Outcomes and terminal status

`outcome.state` is `unknown`, `rejected`, or `accepted`:

- `unknown` omits outcome identity and acceptance evidence;
- `rejected` requires typed `outcome_id` and canonical `outcome_ref`, listed as
  `kind: "outcome"`, and forbids an acceptance claim;
- `accepted` requires outcome identity plus distinct listed acceptance evidence.

Terminal `status` is `succeeded`, `failed`, `rejected`, `cancelled`, or
`timed_out`.  A rejected outcome requires rejected status; an accepted outcome
requires succeeded status.  A failed, cancelled, or timed-out execution remains
a valid record but must not fabricate usage, economics, or acceptance.

Late outcomes append a new receipt or outcome link; already signed bytes are
never rewritten.  Existing `ExecutionReceiptV1`, Context Kernel receipts,
Savings Ledger records, and evidence bundles remain readable.  An adapter may
emit v1 only when every required lineage and evidence link is known; otherwise
the legacy record is a compatibility view and cannot make a v1 verified claim.
