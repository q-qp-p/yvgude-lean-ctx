//! Canonical digest manifest for one admitted Engine invocation.
//!
//! The manifest is a small, strict join document.  It records no payloads:
//! Engine receipts, sources, policies, and capability manifests remain
//! separately persisted and are resolved by their digest bindings.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    CapabilityId, ProtocolReference, SemanticVersion, Sha256Digest, ValidationError,
    deserialize_schema_version, validate_bounded_opaque_identifier, validate_schema_version,
};

/// Maximum number of bindings of each manifest collection.
pub const MAX_INVOCATION_EVIDENCE_ITEMS: usize = 64;

/// Source role in the invocation's complete source lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationSourceRoleV1 {
    /// The one exact input consumed by the Engine.
    Input,
    /// A further source made available to the Engine invocation.
    Context,
}

/// One source locator bound to the digest of its exact persisted bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationSourceBindingV1 {
    pub source_ref: ProtocolReference,
    pub digest: Sha256Digest,
    pub role: InvocationSourceRoleV1,
}

/// Required policy role in the invocation evidence join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationPolicyRoleV1 {
    TaskRegion,
    TaskModel,
    PlanDecision,
    InvocationAdmission,
}

/// One policy locator bound to the digest of the exact policy decision bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationPolicyBindingV1 {
    pub policy_ref: ProtocolReference,
    pub digest: Sha256Digest,
    pub role: InvocationPolicyRoleV1,
}

/// One selected capability bound to its canonical CapabilityManifest bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationCapabilityBindingV1 {
    pub capability_id: CapabilityId,
    pub capability_version: SemanticVersion,
    pub manifest_digest: Sha256Digest,
}

/// Exact Engine receipt locator and content digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationEngineReceiptBindingV1 {
    /// Must be exactly `receipt:sha256:<64 lowercase hex characters>`.
    pub receipt_ref: ProtocolReference,
    /// Digest of the exact Engine receipt bytes.
    pub receipt_digest: Sha256Digest,
}

impl InvocationEngineReceiptBindingV1 {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_manifest_reference(&self.receipt_ref, "receipt_ref")?;
        let expected = format!("receipt:{}", self.receipt_digest.as_str());
        if self.receipt_ref.as_str() != expected {
            return Err(ValidationError::new(
                "InvocationEngineReceiptBindingV1 receipt_ref must equal receipt:<receipt_digest>",
            ));
        }
        Ok(())
    }
}

fn validate_manifest_reference(
    reference: &ProtocolReference,
    field: &str,
) -> Result<(), ValidationError> {
    reject_manifest_feff(reference.as_str(), field)
}

fn reject_manifest_feff(value: &str, field: &str) -> Result<(), ValidationError> {
    if value.contains('\u{feff}') {
        return Err(ValidationError::new(format!(
            "{field} must not contain U+FEFF"
        )));
    }
    Ok(())
}

/// Strict, canonical evidence join for one Engine invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationEvidenceManifestV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    /// Canonical digest of the Engine invocation record.
    pub invocation_ref: Sha256Digest,
    pub engine_receipt: InvocationEngineReceiptBindingV1,
    /// Complete source lineage; exactly one binding has role `input`.
    pub source_bindings: Vec<InvocationSourceBindingV1>,
    /// One or more policy bindings; `invocation_admission` is always present.
    pub policy_bindings: Vec<InvocationPolicyBindingV1>,
    /// Selected capability ID/version to canonical manifest digest bindings.
    pub capability_bindings: Vec<InvocationCapabilityBindingV1>,
}

impl InvocationEvidenceManifestV1 {
    /// Schema version represented by this type.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Return compact canonical JSON bytes for this manifest.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate()?;
        canonical_value(self)
    }

    /// Return canonical JSON as UTF-8 text.
    pub fn canonical_json(&self) -> Result<String, ValidationError> {
        String::from_utf8(self.canonical_bytes()?)
            .map_err(|error| ValidationError::new(format!("manifest canonical UTF-8: {error}")))
    }

    /// Decode exactly canonical JSON and validate every semantic binding.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ValidationError> {
        let value = strict_json_value(bytes)?;
        let manifest = serde_json::from_value::<Self>(value).map_err(|error| {
            ValidationError::new(format!("decode invocation evidence manifest: {error}"))
        })?;
        if manifest.canonical_bytes()? != bytes {
            return Err(ValidationError::new(
                "invocation evidence manifest JSON is not canonical UTF-8, compactness or key order",
            ));
        }
        Ok(manifest)
    }

    /// Compatibility alias for callers naming the input `from_bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidationError> {
        Self::from_canonical_bytes(bytes)
    }

    /// Validate schema, bounds, uniqueness, and cross-binding invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        self.engine_receipt.validate()?;
        validate_source_bindings(&self.source_bindings)?;
        validate_policy_bindings(&self.policy_bindings)?;
        validate_capability_bindings(&self.capability_bindings)?;
        Ok(())
    }
}

fn validate_source_bindings(bindings: &[InvocationSourceBindingV1]) -> Result<(), ValidationError> {
    if bindings.is_empty() || bindings.len() > MAX_INVOCATION_EVIDENCE_ITEMS {
        return Err(ValidationError::new(format!(
            "source_bindings must contain 1..={MAX_INVOCATION_EVIDENCE_ITEMS} entries"
        )));
    }
    let mut refs = BTreeSet::new();
    let mut digests = BTreeSet::new();
    let mut input_count = 0usize;
    for binding in bindings {
        validate_manifest_reference(&binding.source_ref, "source_ref")?;
        if binding.source_ref.as_str().trim().is_empty() {
            return Err(ValidationError::new(
                "InvocationSourceBindingV1 source_ref must not be empty",
            ));
        }
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

fn validate_policy_bindings(bindings: &[InvocationPolicyBindingV1]) -> Result<(), ValidationError> {
    if bindings.is_empty() || bindings.len() > 4 {
        return Err(ValidationError::new(
            "policy_bindings must contain 1..=4 entries",
        ));
    }
    let mut refs = BTreeSet::new();
    let mut digests = BTreeSet::new();
    let mut roles = BTreeSet::new();
    for binding in bindings {
        validate_manifest_reference(&binding.policy_ref, "policy_ref")?;
        if binding.policy_ref.as_str().trim().is_empty() {
            return Err(ValidationError::new(
                "InvocationPolicyBindingV1 policy_ref must not be empty",
            ));
        }
        if !refs.insert(binding.policy_ref.as_str()) {
            return Err(ValidationError::new(
                "policy_bindings policy_ref values must be unique",
            ));
        }
        if !digests.insert(binding.digest.as_str()) {
            return Err(ValidationError::new(
                "policy_bindings digest values must be unique",
            ));
        }
        if !roles.insert(binding.role) {
            return Err(ValidationError::new(
                "policy_bindings role values must be unique",
            ));
        }
    }
    if !roles.contains(&InvocationPolicyRoleV1::InvocationAdmission) {
        return Err(ValidationError::new(
            "policy_bindings must include exactly one invocation_admission role",
        ));
    }
    Ok(())
}

fn validate_capability_bindings(
    bindings: &[InvocationCapabilityBindingV1],
) -> Result<(), ValidationError> {
    if bindings.is_empty() || bindings.len() > MAX_INVOCATION_EVIDENCE_ITEMS {
        return Err(ValidationError::new(format!(
            "capability_bindings must contain 1..={MAX_INVOCATION_EVIDENCE_ITEMS} entries"
        )));
    }
    let mut keys = BTreeSet::new();
    let mut digests = BTreeSet::new();
    for binding in bindings {
        reject_manifest_feff(binding.capability_id.as_str(), "capability_id")?;
        validate_bounded_opaque_identifier(
            binding.capability_id.as_str(),
            "InvocationCapabilityBindingV1 capability_id",
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
    }
    Ok(())
}

fn canonical_value<T: Serialize>(value: &T) -> Result<Vec<u8>, ValidationError> {
    let value = serde_json::to_value(value).map_err(|error| {
        ValidationError::new(format!("serialize invocation evidence manifest: {error}"))
    })?;
    serde_json::to_vec(&sort_json(value)).map_err(|error| {
        ValidationError::new(format!(
            "canonicalize invocation evidence manifest: {error}"
        ))
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

/// Serde visitor rejecting duplicate keys at every JSON object depth.
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
            "floating-point JSON numbers are not canonical manifest values",
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
        .map_err(|error| ValidationError::new(format!("decode canonical manifest JSON: {error}")))?
        .0;
    deserializer.end().map_err(|error| {
        ValidationError::new(format!("trailing canonical manifest JSON: {error}"))
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(hex: char) -> Sha256Digest {
        Sha256Digest::new(format!("sha256:{}", hex.to_string().repeat(64)))
            .expect("fixture digest should be valid")
    }

    fn reference(value: &str) -> ProtocolReference {
        ProtocolReference::new(value).expect("fixture reference should be valid")
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::new(value).expect("fixture capability should be valid")
    }

    fn version(value: &str) -> SemanticVersion {
        SemanticVersion::new(value).expect("fixture version should be valid")
    }

    fn valid_manifest() -> InvocationEvidenceManifestV1 {
        InvocationEvidenceManifestV1 {
            schema_version: 1,
            invocation_ref: digest('a'),
            engine_receipt: InvocationEngineReceiptBindingV1 {
                receipt_ref: reference(&format!("receipt:sha256:{}", "b".repeat(64))),
                receipt_digest: digest('b'),
            },
            source_bindings: vec![
                InvocationSourceBindingV1 {
                    source_ref: reference("source:input"),
                    digest: digest('c'),
                    role: InvocationSourceRoleV1::Input,
                },
                InvocationSourceBindingV1 {
                    source_ref: reference("source:context"),
                    digest: digest('d'),
                    role: InvocationSourceRoleV1::Context,
                },
            ],
            policy_bindings: vec![
                InvocationPolicyBindingV1 {
                    policy_ref: reference("policy:task-region"),
                    digest: digest('e'),
                    role: InvocationPolicyRoleV1::TaskRegion,
                },
                InvocationPolicyBindingV1 {
                    policy_ref: reference("policy:task-model"),
                    digest: digest('f'),
                    role: InvocationPolicyRoleV1::TaskModel,
                },
                InvocationPolicyBindingV1 {
                    policy_ref: reference("policy:plan-decision"),
                    digest: digest('1'),
                    role: InvocationPolicyRoleV1::PlanDecision,
                },
                InvocationPolicyBindingV1 {
                    policy_ref: reference("policy:invocation-admission"),
                    digest: digest('2'),
                    role: InvocationPolicyRoleV1::InvocationAdmission,
                },
            ],
            capability_bindings: vec![InvocationCapabilityBindingV1 {
                capability_id: capability("capability:engine"),
                capability_version: version("1.0.0"),
                manifest_digest: digest('3'),
            }],
        }
    }

    #[test]
    fn valid_manifest_round_trips_and_is_canonical() {
        let manifest = valid_manifest();
        let bytes = manifest.canonical_bytes().expect("manifest canonicalizes");
        let decoded = InvocationEvidenceManifestV1::from_canonical_bytes(&bytes)
            .expect("canonical bytes decode");
        assert_eq!(manifest, decoded);
        assert_eq!(
            serde_json::to_vec(&serde_json::from_slice::<Value>(&bytes).unwrap())
                .unwrap()
                .len(),
            bytes.len()
        );
    }

    #[test]
    fn multibyte_reference_within_byte_bound_is_valid() {
        let mut manifest = valid_manifest();
        manifest.source_bindings[0].source_ref = reference("source:é");
        manifest
            .validate()
            .expect("short multibyte reference is valid");
        assert!(CapabilityId::new("é".repeat(128)).is_ok());
        assert!(CapabilityId::new("é".repeat(129)).is_err());
    }

    #[test]
    fn optional_policy_roles_are_valid_but_admission_is_mandatory() {
        let mut manifest = valid_manifest();
        manifest
            .policy_bindings
            .retain(|binding| binding.role == InvocationPolicyRoleV1::InvocationAdmission);
        manifest
            .validate()
            .expect("admission-only policy set is valid");
        manifest.policy_bindings.clear();
        assert!(manifest.validate().is_err());
        manifest.policy_bindings.push(InvocationPolicyBindingV1 {
            policy_ref: reference("policy:task-region"),
            digest: digest('e'),
            role: InvocationPolicyRoleV1::TaskRegion,
        });
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn shared_references_reject_whitespace_only_and_c1_controls() {
        assert!(ProtocolReference::new(" \t\n").is_err());
        assert!(ProtocolReference::new("\u{0085}").is_err());
        assert!(CapabilityId::new(" \t\n").is_err());
        assert!(CapabilityId::new("\u{0085}").is_err());
    }

    #[test]
    fn manifest_protocol_references_reject_feff_on_all_paths() {
        let mut manifest = valid_manifest();
        manifest.source_bindings[0].source_ref = reference("source:\u{feff}");
        assert!(manifest.validate().is_err());

        let mut manifest = valid_manifest();
        manifest.policy_bindings[0].policy_ref = reference("policy:\u{feff}");
        assert!(manifest.validate().is_err());

        let mut manifest = valid_manifest();
        manifest.engine_receipt.receipt_ref = reference(&format!(
            "receipt:\u{feff}{}",
            manifest.engine_receipt.receipt_digest.as_str()
        ));
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_capability_ids_reject_feff() {
        let mut manifest = valid_manifest();
        manifest.capability_bindings[0].capability_id = capability("capability:\u{feff}");
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn canonical_decoder_rejects_whitespace_key_order_and_duplicates() {
        let canonical = valid_manifest().canonical_bytes().expect("canonicalizes");
        let pretty =
            serde_json::to_vec_pretty(&serde_json::from_slice::<Value>(&canonical).unwrap())
                .expect("pretty JSON");
        assert!(InvocationEvidenceManifestV1::from_canonical_bytes(&pretty).is_err());

        let mut reordered = canonical.clone();
        reordered.reverse();
        assert!(InvocationEvidenceManifestV1::from_canonical_bytes(&reordered).is_err());

        let duplicate = br#"{"schema_version":1,"schema_version":1}"#;
        assert!(InvocationEvidenceManifestV1::from_canonical_bytes(duplicate).is_err());
    }

    #[test]
    fn validation_rejects_receipt_ref_mismatch() {
        let mut manifest = valid_manifest();
        manifest.engine_receipt.receipt_ref = reference(
            "receipt:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validation_rejects_missing_or_duplicate_input() {
        let mut manifest = valid_manifest();
        manifest.source_bindings[0].role = InvocationSourceRoleV1::Context;
        assert!(manifest.validate().is_err());
        manifest.source_bindings[0].role = InvocationSourceRoleV1::Input;
        manifest.source_bindings[1].role = InvocationSourceRoleV1::Input;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validation_rejects_policy_role_gaps_and_duplicates() {
        let mut manifest = valid_manifest();
        manifest.policy_bindings[0].role = InvocationPolicyRoleV1::TaskModel;
        assert!(manifest.validate().is_err());
        manifest = valid_manifest();
        manifest.policy_bindings[0].policy_ref = manifest.policy_bindings[1].policy_ref.clone();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validation_rejects_duplicate_capability_key() {
        let mut manifest = valid_manifest();
        manifest
            .capability_bindings
            .push(manifest.capability_bindings[0].clone());
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn unknown_fields_are_denied_at_every_binding_level() {
        let mut value = serde_json::to_value(valid_manifest()).expect("serialize fixture");
        value["engine_receipt"]["future"] = Value::Bool(true);
        assert!(serde_json::from_value::<InvocationEvidenceManifestV1>(value).is_err());
    }

    #[test]
    fn schema_declares_mandatory_three_stage_conformance() {
        let schema: Value = serde_json::from_slice(include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest-v1.schema.json"
        ))
        .expect("manifest schema should be valid JSON");
        assert_eq!(schema["x-conformance"]["mode"], "three_stage");
        assert_eq!(schema["x-conformance"]["schema_validation_required"], true);
        assert_eq!(
            schema["x-conformance"]["semantic_validation_required"],
            true
        );
        assert_eq!(
            schema["x-conformance"]["utf8_bounds"]["keyword"],
            "x-maxUtf8Bytes"
        );
        assert_eq!(
            schema["x-conformance"]["utf8_bounds"]["schema_enforcement"],
            "annotation_only"
        );
        assert_eq!(
            schema["x-conformance"]["cross_artifact_join_required"],
            true
        );
        assert_eq!(
            schema["x-conformance"]["stages"][2]["name"],
            "cross_artifact_join"
        );
        assert_eq!(schema["x-conformance"]["stages"][2]["required"], true);
        assert_eq!(
            schema["x-conformance"]["stages"][2]["protocol_decoder_sufficient"],
            false
        );
        let requirements = schema["x-conformance"]["stages"][2]["requirements"]
            .as_array()
            .expect("cross-artifact requirements should be an array");
        assert!(
            requirements
                .iter()
                .any(|requirement| { requirement == "resolve_and_verify_policy_artifact_bytes" })
        );
    }

    #[test]
    fn canonical_golden_and_negative_fixtures_are_exhaustive() {
        let valid =
            include_bytes!("../../../../docs/contracts/invocation-evidence-manifest/v1/valid.json");
        let valid = valid.strip_suffix(b"\n").unwrap_or(valid);
        let canonical = include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest/v1/canonical.json"
        );
        let canonical = canonical.strip_suffix(b"\n").unwrap_or(canonical);
        let decoded = InvocationEvidenceManifestV1::from_canonical_bytes(valid)
            .expect("valid fixture should decode canonically");
        assert_eq!(
            decoded.canonical_bytes().expect("canonical bytes"),
            canonical
        );
        let optional = include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest/v1/valid-optional-policy.json"
        );
        let optional = optional.strip_suffix(b"\n").unwrap_or(optional);
        let optional_manifest = InvocationEvidenceManifestV1::from_canonical_bytes(optional)
            .expect("admission-only policy fixture should decode");
        assert_eq!(optional_manifest.policy_bindings.len(), 1);
        let multibyte = include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest/v1/valid-multibyte.json"
        );
        let multibyte = multibyte.strip_suffix(b"\n").unwrap_or(multibyte);
        InvocationEvidenceManifestV1::from_canonical_bytes(multibyte)
            .expect("short multibyte reference fixture should decode");

        for (name, expected) in [
            ("valid-u2028-reference.json", "source:\u{2028}value"),
            ("valid-u2029-reference.json", "policy:\u{2029}decision"),
        ] {
            let bytes: &[u8] = match name {
                "valid-u2028-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/valid-u2028-reference.json"
                ),
                "valid-u2029-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/valid-u2029-reference.json"
                ),
                _ => unreachable!(),
            };
            let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
            let manifest = InvocationEvidenceManifestV1::from_canonical_bytes(bytes)
                .expect("U+2028/U+2029 reference fixture should decode");
            assert!(
                manifest
                    .source_bindings
                    .iter()
                    .any(|binding| binding.source_ref.as_str() == expected)
                    || manifest
                        .policy_bindings
                        .iter()
                        .any(|binding| binding.policy_ref.as_str() == expected)
            );
        }

        let exact_reference = include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest/v1/valid-reference-1024.json"
        );
        let exact_reference = exact_reference
            .strip_suffix(b"\n")
            .unwrap_or(exact_reference);
        let exact_manifest = InvocationEvidenceManifestV1::from_canonical_bytes(exact_reference)
            .expect("exact 1024-byte ProtocolReference fixture should decode");
        assert_eq!(
            exact_manifest.source_bindings[0].source_ref.as_str().len(),
            1024
        );

        let overlong = include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-utf8-byte-bound.json"
        );
        let overlong = overlong.strip_suffix(b"\n").unwrap_or(overlong);
        assert!(InvocationEvidenceManifestV1::from_canonical_bytes(overlong).is_err());

        let overlong_reference = include_bytes!(
            "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-reference-1025.json"
        );
        let overlong_reference = overlong_reference
            .strip_suffix(b"\n")
            .unwrap_or(overlong_reference);
        assert!(InvocationEvidenceManifestV1::from_canonical_bytes(overlong_reference).is_err());

        for name in [
            "invalid-unknown-field.json",
            "invalid-receipt-ref-mismatch.json",
            "invalid-source-input-count.json",
            "invalid-policy-role.json",
            "invalid-capability-key.json",
            "invalid-duplicate-source-ref.json",
            "invalid-duplicate-source-digest.json",
            "invalid-duplicate-policy-digest.json",
            "invalid-duplicate-policy-role.json",
            "invalid-duplicate-policy-ref.json",
            "invalid-duplicate-capability-manifest-digest.json",
            "invalid-missing-invocation-admission.json",
            "invalid-whitespace-reference.json",
            "invalid-c1-control-reference.json",
            "invalid-feff-source-reference.json",
            "invalid-feff-policy-reference.json",
            "invalid-feff-receipt-reference.json",
            "invalid-feff-capability-id.json",
            "invalid-schema-version-float.json",
        ] {
            let bytes: &[u8] = match name {
                "invalid-unknown-field.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-unknown-field.json"
                ),
                "invalid-receipt-ref-mismatch.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-receipt-ref-mismatch.json"
                ),
                "invalid-source-input-count.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-source-input-count.json"
                ),
                "invalid-policy-role.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-policy-role.json"
                ),
                "invalid-capability-key.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-capability-key.json"
                ),
                "invalid-duplicate-source-ref.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-duplicate-source-ref.json"
                ),
                "invalid-duplicate-source-digest.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-duplicate-source-digest.json"
                ),
                "invalid-duplicate-policy-digest.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-duplicate-policy-digest.json"
                ),
                "invalid-duplicate-policy-role.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-duplicate-policy-role.json"
                ),
                "invalid-duplicate-policy-ref.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-duplicate-policy-ref.json"
                ),
                "invalid-duplicate-capability-manifest-digest.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-duplicate-capability-manifest-digest.json"
                ),
                "invalid-missing-invocation-admission.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-missing-invocation-admission.json"
                ),
                "invalid-whitespace-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-whitespace-reference.json"
                ),
                "invalid-c1-control-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-c1-control-reference.json"
                ),
                "invalid-feff-source-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-feff-source-reference.json"
                ),
                "invalid-feff-policy-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-feff-policy-reference.json"
                ),
                "invalid-feff-receipt-reference.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-feff-receipt-reference.json"
                ),
                "invalid-feff-capability-id.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-feff-capability-id.json"
                ),
                "invalid-schema-version-float.json" => include_bytes!(
                    "../../../../docs/contracts/invocation-evidence-manifest/v1/invalid-schema-version-float.json"
                ),
                _ => unreachable!(),
            };
            let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
            assert!(
                InvocationEvidenceManifestV1::from_canonical_bytes(bytes).is_err(),
                "{name} must fail closed"
            );
        }
    }
}
