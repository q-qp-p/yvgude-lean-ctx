//! Strict, canonical signed admission context for one Engine invocation.
//!
//! `InvocationContextBindingV1` is a sidecar to the invocation and evidence
//! manifest.  It binds the session, task, plan, invocation, admission policy,
//! complete source lineage, and selected capabilities without embedding any
//! of their payload bytes.  Trust-store lookup, revocation, one-time
//! consumption, and wall-clock admission checks belong to the runtime gate.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    DecisionId, EngineInvocationIdV1, EnginePolicyDecisionV1, InvocationCapabilityBindingV1,
    InvocationSourceBindingV1, InvocationSourceRoleV1, PlanId, ProtocolReference, Sha256Digest,
    TaskId, UtcTimestamp, ValidationError, deserialize_schema_version,
    validate_bounded_opaque_identifier, validate_schema_version,
};

/// Domain prefix covered by an InvocationContextBindingV1 signature.
pub const INVOCATION_CONTEXT_BINDING_SIGNATURE_DOMAIN: &[u8] =
    b"leanctx/invocation-context-binding/v1\0";

/// Maximum source and capability entries in one binding.
pub const MAX_INVOCATION_CONTEXT_BINDING_ITEMS: usize = 64;

/// Metadata identifying the key used to sign an invocation context binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationContextBindingSignerV1 {
    pub algorithm: String,
    pub key_id: String,
    pub public_key_digest: Sha256Digest,
}

/// Canonical signed admission context for one Engine invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationContextBindingV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub admission_id: DecisionId,
    pub session_identity_ref: Sha256Digest,
    pub task_id: TaskId,
    pub task_ref: Sha256Digest,
    pub plan_id: PlanId,
    pub plan_ref: Sha256Digest,
    pub invocation_id: EngineInvocationIdV1,
    pub invocation_ref: Sha256Digest,
    pub policy_ref: ProtocolReference,
    pub policy_digest: Sha256Digest,
    pub decision: EnginePolicyDecisionV1,
    pub source_bindings: Vec<InvocationSourceBindingV1>,
    pub capability_bindings: Vec<InvocationCapabilityBindingV1>,
    pub issued_at: UtcTimestamp,
    pub not_before: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub signer: InvocationContextBindingSignerV1,
    pub signature: String,
}

impl InvocationContextBindingV1 {
    /// Schema version represented by this type.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Return canonical compact JSON bytes including the signature.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate()?;
        canonical_value_without_signature(self, false)
    }

    /// Return canonical JSON as UTF-8 text including the signature.
    pub fn canonical_json(&self) -> Result<String, ValidationError> {
        String::from_utf8(self.canonical_bytes()?).map_err(|error| {
            ValidationError::new(format!(
                "invocation context binding canonical UTF-8: {error}"
            ))
        })
    }

    /// Return canonical JSON bytes covered by the signature, without signature.
    pub fn unsigned_canonical_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate_unsigned()?;
        canonical_value_without_signature(self, true)
    }

    /// Return the signature payload, including the versioned domain prefix.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        let unsigned = self.unsigned_canonical_bytes()?;
        let mut bytes =
            Vec::with_capacity(INVOCATION_CONTEXT_BINDING_SIGNATURE_DOMAIN.len() + unsigned.len());
        bytes.extend_from_slice(INVOCATION_CONTEXT_BINDING_SIGNATURE_DOMAIN);
        bytes.extend_from_slice(&unsigned);
        Ok(bytes)
    }

    /// Alias for callers that name signature coverage bytes explicitly.
    pub fn signature_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        self.signing_bytes()
    }

    /// Digest the complete canonical binding bytes, including its signature.
    pub fn digest(&self) -> Result<Sha256Digest, ValidationError> {
        let bytes = self.canonical_bytes()?;
        digest_bytes(&bytes)
    }

    /// Compatibility alias for callers naming the content identity explicitly.
    pub fn derived_binding_digest(&self) -> Result<Sha256Digest, ValidationError> {
        self.digest()
    }

    /// Decode exactly canonical JSON and validate every binding invariant.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ValidationError> {
        let value = strict_json_value(bytes)?;
        let binding = serde_json::from_value::<Self>(value).map_err(|error| {
            ValidationError::new(format!("decode invocation context binding: {error}"))
        })?;
        if binding.canonical_bytes()? != bytes {
            return Err(ValidationError::new(
                "invocation context binding JSON is not canonical UTF-8, compactness or key order",
            ));
        }
        Ok(binding)
    }

    /// Compatibility alias for callers naming the wire input `from_bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidationError> {
        Self::from_canonical_bytes(bytes)
    }

    /// Validate all semantic and canonical-signature metadata invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_unsigned()?;
        validate_base64_signature(&self.signature)?;
        Ok(())
    }

    fn validate_unsigned(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        reject_binding_feff(self.admission_id.as_str(), "admission_id")?;
        validate_bounded_opaque_identifier(self.admission_id.as_str(), "admission_id")?;
        reject_binding_feff(self.task_id.as_str(), "task_id")?;
        reject_binding_feff(self.plan_id.as_str(), "plan_id")?;
        reject_binding_feff(self.invocation_id.as_str(), "invocation_id")?;
        validate_schema_digest(&self.session_identity_ref, "session_identity_ref")?;
        validate_schema_digest(&self.task_ref, "task_ref")?;
        validate_schema_digest(&self.plan_ref, "plan_ref")?;
        validate_schema_digest(&self.invocation_ref, "invocation_ref")?;
        validate_manifest_reference(&self.policy_ref, "policy_ref")?;
        validate_schema_digest(&self.policy_digest, "policy_digest")?;
        if self.decision != EnginePolicyDecisionV1::Admitted {
            return Err(ValidationError::new(
                "InvocationContextBindingV1 decision must be admitted",
            ));
        }
        validate_source_bindings(&self.source_bindings)?;
        validate_capability_bindings(&self.capability_bindings)?;
        if self.not_before > self.issued_at {
            return Err(ValidationError::new(
                "InvocationContextBindingV1 not_before must be at or before issued_at",
            ));
        }
        if self.issued_at >= self.expires_at {
            return Err(ValidationError::new(
                "InvocationContextBindingV1 issued_at must be before expires_at",
            ));
        }
        validate_signer(&self.signer)?;
        Ok(())
    }
}

fn validate_signer(signer: &InvocationContextBindingSignerV1) -> Result<(), ValidationError> {
    if signer.algorithm != "ed25519" {
        return Err(ValidationError::new(
            "InvocationContextBindingSignerV1 algorithm must be ed25519",
        ));
    }
    reject_binding_feff(&signer.key_id, "key_id")?;
    validate_key_id(&signer.key_id)?;
    // Sha256Digest is exactly 32 bytes represented by 64 lowercase hex bytes;
    // retaining it as a typed field prevents embedding a public key or a
    // variable-length key digest in this metadata-only contract.
    validate_schema_digest(&signer.public_key_digest, "public_key_digest")
}

fn reject_binding_feff(value: &str, field: &str) -> Result<(), ValidationError> {
    if value.contains('\u{feff}') {
        return Err(ValidationError::new(format!(
            "{field} must not contain U+FEFF"
        )));
    }
    Ok(())
}

fn validate_key_id(value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
        || value.starts_with("base64:")
        || value.starts_with("hex:")
    {
        return Err(ValidationError::new(
            "InvocationContextBindingSignerV1 key_id must be a bounded identifier, not key material",
        ));
    }
    Ok(())
}

fn validate_schema_digest(value: &Sha256Digest, field: &str) -> Result<(), ValidationError> {
    if value.as_str().len() != "sha256:".len() + 64 {
        return Err(ValidationError::new(format!(
            "{field} must be a SHA-256 digest"
        )));
    }
    Ok(())
}

fn validate_manifest_reference(
    reference: &ProtocolReference,
    field: &str,
) -> Result<(), ValidationError> {
    if reference.as_str().contains('\u{feff}') {
        return Err(ValidationError::new(format!(
            "{field} must not contain U+FEFF"
        )));
    }
    if reference.as_str().trim().is_empty() {
        return Err(ValidationError::new(format!("{field} must not be empty")));
    }
    Ok(())
}

fn validate_source_bindings(bindings: &[InvocationSourceBindingV1]) -> Result<(), ValidationError> {
    if bindings.is_empty() || bindings.len() > MAX_INVOCATION_CONTEXT_BINDING_ITEMS {
        return Err(ValidationError::new(format!(
            "source_bindings must contain 1..={MAX_INVOCATION_CONTEXT_BINDING_ITEMS} entries"
        )));
    }
    let mut refs = BTreeSet::new();
    let mut digests = BTreeSet::new();
    let mut input_count = 0usize;
    let mut previous_ref = None;
    for binding in bindings {
        validate_manifest_reference(&binding.source_ref, "source_ref")?;
        if !refs.insert(binding.source_ref.as_str()) {
            return Err(ValidationError::new(
                "source_bindings source_ref values must be unique",
            ));
        }
        if !digests.insert(binding.digest.as_str()) {
            return Err(ValidationError::new(
                "source_bindings digest values must be unique",
            ));
        }
        if let Some(previous) = previous_ref {
            if previous >= binding.source_ref.as_str() {
                return Err(ValidationError::new(
                    "source_bindings must be sorted by source_ref",
                ));
            }
        }
        previous_ref = Some(binding.source_ref.as_str());
        if binding.role == InvocationSourceRoleV1::Input {
            input_count += 1;
        }
    }
    if input_count != 1 {
        return Err(ValidationError::new(
            "source_bindings must contain exactly one input",
        ));
    }
    Ok(())
}

fn validate_capability_bindings(
    bindings: &[InvocationCapabilityBindingV1],
) -> Result<(), ValidationError> {
    if bindings.len() != 1 {
        return Err(ValidationError::new(format!(
            "capability_bindings must contain exactly one entry in V1 (maximum {MAX_INVOCATION_CONTEXT_BINDING_ITEMS})"
        )));
    }
    let mut keys = BTreeSet::new();
    let mut digests = BTreeSet::new();
    let mut previous_key = None;
    for binding in bindings {
        validate_manifest_reference(
            &ProtocolReference::new(binding.capability_id.as_str().to_owned())?,
            "capability_id",
        )?;
        let key = (
            binding.capability_id.as_str(),
            binding.capability_version.as_str(),
        );
        if !keys.insert(key) {
            return Err(ValidationError::new(
                "capability_bindings capability_id/version pairs must be unique",
            ));
        }
        if !digests.insert(binding.manifest_digest.as_str()) {
            return Err(ValidationError::new(
                "capability_bindings manifest_digest values must be unique",
            ));
        }
        let ordering_key = (
            binding.capability_id.as_str(),
            binding.capability_version.as_str(),
        );
        if let Some(previous) = previous_key {
            if previous >= ordering_key {
                return Err(ValidationError::new(
                    "capability_bindings must be sorted by capability_id and capability_version",
                ));
            }
        }
        previous_key = Some(ordering_key);
    }
    Ok(())
}

fn validate_base64_signature(value: &str) -> Result<(), ValidationError> {
    if value.len() != 88 || !value.ends_with("==") {
        return Err(ValidationError::new(
            "InvocationContextBindingV1 signature must be canonical base64 Ed25519 bytes",
        ));
    }
    let bytes = value.as_bytes();
    if !bytes[..86]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'+' || *byte == b'/')
    {
        return Err(ValidationError::new(
            "InvocationContextBindingV1 signature contains invalid base64 characters",
        ));
    }
    // 64 bytes encode as 86 meaningful characters plus `==`; low four bits of
    // the final sextet are padding and must be zero for one canonical spelling.
    if base64_value(bytes[85]).is_none_or(|last| last & 0x0f != 0) {
        return Err(ValidationError::new(
            "InvocationContextBindingV1 signature has non-zero base64 pad bits",
        ));
    }
    Ok(())
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn canonical_value_without_signature<T: Serialize>(
    value: &T,
    omit_signature: bool,
) -> Result<Vec<u8>, ValidationError> {
    let mut value = serde_json::to_value(value).map_err(|error| {
        ValidationError::new(format!("serialize invocation context binding: {error}"))
    })?;
    if omit_signature {
        let Value::Object(object) = &mut value else {
            return Err(ValidationError::new(
                "invocation context binding must serialize as a JSON object",
            ));
        };
        object.remove("signature");
    }
    serde_json::to_vec(&sort_json(value)).map_err(|error| {
        ValidationError::new(format!("canonicalize invocation context binding: {error}"))
    })
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        scalar => scalar,
    }
}

/// Serde visitor rejecting duplicate keys and floating-point numbers.
struct StrictJsonValue(Value);

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a canonical JSON value with unique object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(
            "floating-point JSON numbers are not canonical binding values",
        ))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element::<StrictJsonValue>()? {
            values.push(value.0);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON object key {key:?}"
                )));
            }
            let value = map.next_value::<StrictJsonValue>()?;
            object.insert(key, value.0);
        }
        Ok(StrictJsonValue(Value::Object(object)))
    }
}

fn strict_json_value(bytes: &[u8]) -> Result<Value, ValidationError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictJsonValue::deserialize(&mut deserializer)
        .map_err(|error| ValidationError::new(format!("decode canonical binding JSON: {error}")))?
        .0;
    deserializer.end().map_err(|error| {
        ValidationError::new(format!("trailing canonical binding JSON: {error}"))
    })?;
    Ok(value)
}

fn digest_bytes(bytes: &[u8]) -> Result<Sha256Digest, ValidationError> {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity("sha256:".len() + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}")
            .map_err(|error| ValidationError::new(format!("encode binding digest: {error}")))?;
    }
    Sha256Digest::new(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(ch: char) -> Sha256Digest {
        Sha256Digest::new(format!("sha256:{}", ch.to_string().repeat(64))).unwrap()
    }

    fn source_ref(value: &str) -> ProtocolReference {
        ProtocolReference::new(value).unwrap()
    }

    fn valid_binding() -> InvocationContextBindingV1 {
        InvocationContextBindingV1 {
            schema_version: 1,
            admission_id: DecisionId::new("admission-1").unwrap(),
            session_identity_ref: digest('a'),
            task_id: TaskId::new("task-1").unwrap(),
            task_ref: digest('b'),
            plan_id: PlanId::new("plan-1").unwrap(),
            plan_ref: digest('c'),
            invocation_id: EngineInvocationIdV1::new("invocation-1").unwrap(),
            invocation_ref: digest('d'),
            policy_ref: source_ref("policy:invocation-admission"),
            policy_digest: digest('e'),
            decision: EnginePolicyDecisionV1::Admitted,
            source_bindings: vec![InvocationSourceBindingV1 {
                source_ref: source_ref("source:input"),
                digest: digest('f'),
                role: InvocationSourceRoleV1::Input,
            }],
            capability_bindings: vec![InvocationCapabilityBindingV1 {
                capability_id: crate::CapabilityId::new("capability:engine").unwrap(),
                capability_version: crate::SemanticVersion::new("1.0.0").unwrap(),
                manifest_digest: digest('1'),
            }],
            issued_at: UtcTimestamp::new("2026-08-23T12:00:00Z").unwrap(),
            not_before: UtcTimestamp::new("2026-08-23T11:59:00Z").unwrap(),
            expires_at: UtcTimestamp::new("2026-08-23T12:05:00Z").unwrap(),
            signer: InvocationContextBindingSignerV1 {
                algorithm: "ed25519".to_owned(),
                key_id: "test-key".to_owned(),
                public_key_digest: digest('2'),
            },
            signature: "A".repeat(86) + "==",
        }
    }

    #[test]
    fn canonical_round_trip_and_signature_domain_are_stable() {
        let binding = valid_binding();
        binding.validate().unwrap();
        let bytes = binding.canonical_bytes().unwrap();
        let decoded = InvocationContextBindingV1::from_canonical_bytes(&bytes).unwrap();
        assert_eq!(decoded, binding);
        let golden =
            include_bytes!("../../../../docs/contracts/invocation-context-binding/v1/valid.json");
        let golden = golden.strip_suffix(b"\n").unwrap_or(golden);
        assert_eq!(
            InvocationContextBindingV1::from_canonical_bytes(golden).unwrap(),
            binding
        );
        let signing = include_bytes!(
            "../../../../docs/contracts/invocation-context-binding/v1/canonical-signing.json"
        );
        let signing = signing.strip_suffix(b"\n").unwrap_or(signing);
        assert_eq!(binding.unsigned_canonical_bytes().unwrap(), signing);
        let signing_bytes = binding.signing_bytes().unwrap();
        assert!(signing_bytes.starts_with(INVOCATION_CONTEXT_BINDING_SIGNATURE_DOMAIN));
        assert_eq!(
            signing_bytes[INVOCATION_CONTEXT_BINDING_SIGNATURE_DOMAIN.len() - 1],
            0
        );
        let expected_hex = include_bytes!(
            "../../../../docs/contracts/invocation-context-binding/v1/signing-bytes.hex"
        );
        let expected_hex = expected_hex.strip_suffix(b"\n").unwrap_or(expected_hex);
        let expected_signing_bytes = decode_hex(expected_hex);
        assert_eq!(signing_bytes, expected_signing_bytes);
        let expected_hash = include_bytes!(
            "../../../../docs/contracts/invocation-context-binding/v1/signing-bytes.sha256"
        );
        let expected_hash = expected_hash.strip_suffix(b"\n").unwrap_or(expected_hash);
        let actual_hash = Sha256::digest(&signing_bytes);
        let actual_hash = actual_hash.iter().fold(String::new(), |mut value, byte| {
            use std::fmt::Write as _;
            write!(value, "{byte:02x}").unwrap();
            value
        });
        assert_eq!(actual_hash.as_bytes(), expected_hash);
        assert_ne!(
            binding.canonical_bytes().unwrap(),
            binding.unsigned_canonical_bytes().unwrap()
        );
        assert_eq!(binding.digest().unwrap(), decoded.digest().unwrap());
    }

    #[test]
    fn strict_decoder_rejects_duplicate_unknown_float_whitespace_and_mutations() {
        let binding = valid_binding();
        let canonical = binding.canonical_bytes().unwrap();
        let mut duplicate = canonical.clone();
        duplicate.pop();
        duplicate.extend_from_slice(b",\"schema_version\":1}");
        assert!(InvocationContextBindingV1::from_canonical_bytes(&duplicate).is_err());

        let mut whitespace = b" \n".to_vec();
        whitespace.extend_from_slice(&canonical);
        assert!(InvocationContextBindingV1::from_canonical_bytes(&whitespace).is_err());
        assert!(strict_json_value(br#"{"schema_version":1.0}"#).is_err());

        let mut unknown = canonical.clone();
        unknown.pop();
        unknown.extend_from_slice(b",\"unknown\":true}");
        assert!(InvocationContextBindingV1::from_canonical_bytes(&unknown).is_err());
    }

    #[test]
    fn signature_metadata_and_admission_invariants_are_strict() {
        let mut binding = valid_binding();
        binding.signature = "A".repeat(85) + "B==";
        assert!(binding.validate().is_err());
        binding = valid_binding();
        binding.signer.algorithm = "ed25519ph".to_owned();
        assert!(binding.validate().is_err());
        binding = valid_binding();
        binding.decision = EnginePolicyDecisionV1::Rejected;
        assert!(binding.validate().is_err());
        binding = valid_binding();
        binding.expires_at = binding.issued_at.clone();
        assert!(binding.validate().is_err());
        binding = valid_binding();
        binding.source_bindings.push(InvocationSourceBindingV1 {
            source_ref: source_ref("source:other"),
            digest: digest('3'),
            role: InvocationSourceRoleV1::Context,
        });
        binding.source_bindings.reverse();
        assert!(binding.validate().is_err());
    }

    #[test]
    fn bounded_ids_reject_feff_on_every_binding_identity_path() {
        let mut binding = valid_binding();
        binding.admission_id = DecisionId::new("admission:\u{feff}").unwrap();
        assert!(binding.validate().is_err());
        let mut binding = valid_binding();
        binding.task_id = TaskId::new("task:\u{feff}").unwrap();
        assert!(binding.validate().is_err());
        let mut binding = valid_binding();
        binding.plan_id = PlanId::new("plan:\u{feff}").unwrap();
        assert!(binding.validate().is_err());
        let mut binding = valid_binding();
        binding.invocation_id = EngineInvocationIdV1::new("invocation:\u{feff}").unwrap();
        assert!(binding.validate().is_err());
        let mut binding = valid_binding();
        binding.signer.key_id = "key:\u{feff}".to_owned();
        assert!(binding.validate().is_err());
    }

    #[test]
    fn wrong_public_key_metadata_changes_coverage_without_trust_claim() {
        let binding = valid_binding();
        let expected = binding.signing_bytes().unwrap();
        let mut wrong_key = binding.clone();
        wrong_key.signer.public_key_digest = digest('3');
        wrong_key.validate().unwrap();
        assert_ne!(wrong_key.signing_bytes().unwrap(), expected);
        // Key lookup and cryptographic verification intentionally remain a
        // caller/runtime gate; protocol validation only checks metadata shape.
    }

    #[test]
    fn capability_order_and_duplicates_are_rejected() {
        let mut binding = valid_binding();
        binding
            .capability_bindings
            .push(InvocationCapabilityBindingV1 {
                capability_id: crate::CapabilityId::new("capability:z").unwrap(),
                capability_version: crate::SemanticVersion::new("1.0.0").unwrap(),
                manifest_digest: digest('4'),
            });
        binding.capability_bindings.reverse();
        assert!(binding.validate().is_err());
        binding.capability_bindings.sort_by(|left, right| {
            (
                left.capability_id.as_str(),
                left.capability_version.as_str(),
            )
                .cmp(&(
                    right.capability_id.as_str(),
                    right.capability_version.as_str(),
                ))
        });
        binding.capability_bindings[1] = binding.capability_bindings[0].clone();
        assert!(binding.validate().is_err());
        binding.capability_bindings[1].manifest_digest = digest('5');
        assert!(binding.validate().is_err());
    }

    #[test]
    fn adversarial_fixture_set_is_rejected() {
        for name in [
            "invalid-duplicate-key.json",
            "invalid-schema-version-float.json",
            "invalid-unknown-field.json",
            "invalid-signature-pad-bits.json",
            "invalid-time-order.json",
            "invalid-duplicate-source.json",
            "invalid-capability-count.json",
            "invalid-feff-task-id.json",
        ] {
            let bytes: &[u8] = match name {
                "invalid-duplicate-key.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-duplicate-key.json"
                ),
                "invalid-schema-version-float.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-schema-version-float.json"
                ),
                "invalid-unknown-field.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-unknown-field.json"
                ),
                "invalid-signature-pad-bits.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-signature-pad-bits.json"
                ),
                "invalid-time-order.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-time-order.json"
                ),
                "invalid-duplicate-source.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-duplicate-source.json"
                ),
                "invalid-capability-count.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-capability-count.json"
                ),
                "invalid-feff-task-id.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-context-binding/v1/invalid-feff-task-id.json"
                ),
                _ => unreachable!(),
            };
            assert!(
                InvocationContextBindingV1::from_canonical_bytes(bytes).is_err(),
                "fixture {name} must be rejected"
            );
        }
    }

    fn decode_hex(bytes: &[u8]) -> Vec<u8> {
        fn nibble(byte: u8) -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("invalid hex fixture"),
            }
        }
        bytes
            .chunks_exact(2)
            .map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1]))
            .collect()
    }

    #[test]
    fn schema_declares_semantic_and_runtime_conformance_boundaries() {
        let schema: Value = serde_json::from_slice(include_bytes!(
            "../../../../docs/contracts/invocation-context-binding-v1.schema.json"
        ))
        .unwrap();
        assert_eq!(schema["x-conformance"]["mode"], "three_stage");
        assert_eq!(
            schema["x-conformance"]["semantic_validation_required"],
            true
        );
        assert_eq!(
            schema["x-conformance"]["cross_artifact_join_required"],
            true
        );
        assert_eq!(schema["x-conformance"]["signature"]["decoded_bytes"], 64);
        assert_eq!(schema["properties"]["capability_bindings"]["minItems"], 1);
        assert_eq!(schema["properties"]["capability_bindings"]["maxItems"], 1);
        let requirements = schema["x-conformance"]["stages"][2]["requirements"]
            .as_array()
            .unwrap();
        assert!(requirements.iter().any(|requirement| {
            requirement
                .as_str()
                .is_some_and(|value| value.contains("policy_digest"))
        }));
        assert!(
            schema["x-conformance"]["runtime_gates_not_in_decoder"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "one_time_consumption")
        );
    }
}
