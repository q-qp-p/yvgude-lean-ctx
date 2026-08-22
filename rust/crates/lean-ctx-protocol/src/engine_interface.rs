//! Narrow, local-only Engine interface records.
//!
//! These records describe one deterministic Engine operation. They deliberately
//! exclude agent sessions, Profiles, Kits, planning, tenancy, Cloud transport,
//! and retry orchestration: a host or future SDK owns those concerns.

use crate::{
    CapabilityId, ProtocolReference, ReceiptId, SemanticVersion, Sha256Digest, ValidationError,
    deserialize_schema_version, validate_bounded_opaque_identifier, validate_schema_version,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};

const MAX_SOURCE_REFS: usize = 32;
const MAX_MEASUREMENTS: usize = 32;
const MAX_SUPPORTED_OPERATIONS: usize = 32;
const MAX_MEASUREMENT_NAME_LENGTH: usize = 64;

/// Opaque identity for one Engine invocation, scoped to the local Engine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct EngineInvocationIdV1(String);

impl EngineInvocationIdV1 {
    /// Construct a bounded opaque invocation identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_bounded_opaque_identifier(&value, "EngineInvocationIdV1")?;
        Ok(Self(value))
    }

    /// Borrow the wire representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EngineInvocationIdV1 {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for EngineInvocationIdV1 {
    type Error = ValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for EngineInvocationIdV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(DeError::custom)
    }
}

/// Resolved local Engine identity; it is not a tenant, user, or agent identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedLocalEngineIdentityV1 {
    pub engine_id: String,
    pub engine_version: SemanticVersion,
}

impl ResolvedLocalEngineIdentityV1 {
    /// Validate local Engine identity fields.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_bounded_opaque_identifier(
            &self.engine_id,
            "ResolvedLocalEngineIdentityV1 engine_id",
        )
    }
}

/// One named, versioned Engine capability selected by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineOperationV1 {
    pub capability_id: CapabilityId,
    pub capability_version: SemanticVersion,
}

/// The result of local policy admission before an Engine operation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnginePolicyDecisionV1 {
    Admitted,
    Rejected,
}

/// Policy reference and admission decision for a single invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnginePolicyAdmissionV1 {
    pub policy_ref: ProtocolReference,
    pub decision: EnginePolicyDecisionV1,
}

/// A deterministic local Engine request after identity and policy resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineInvocationV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub invocation_id: EngineInvocationIdV1,
    pub engine: ResolvedLocalEngineIdentityV1,
    pub operation: EngineOperationV1,
    /// Addressable input retained by the host; raw input is intentionally absent.
    pub input_ref: ProtocolReference,
    /// Digest of the exact bounded input consumed by the Engine.
    pub input_digest: Sha256Digest,
    /// Source lineage available to recovery; includes `input_ref` exactly once.
    pub source_refs: Vec<ProtocolReference>,
    pub policy_admission: EnginePolicyAdmissionV1,
}

impl EngineInvocationV1 {
    /// Validate schema, source lineage, and deterministic input invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        self.engine.validate()?;
        validate_nonempty_unique_refs(&self.source_refs, "EngineInvocationV1 source_refs")?;
        if !self
            .source_refs
            .iter()
            .any(|reference| reference == &self.input_ref)
        {
            return Err(ValidationError::new(
                "EngineInvocationV1 source_refs must contain input_ref",
            ));
        }
        Ok(())
    }
}

/// Whether an observation value was directly measured, derived as an estimate,
/// or unavailable for the invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineValueClassificationV1 {
    Measured,
    Estimated,
    Unavailable,
}

/// One bounded, named numerical Engine observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineMeasurementV1 {
    pub name: String,
    pub unit: String,
    pub classification: EngineValueClassificationV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<u64>,
}

impl EngineMeasurementV1 {
    /// Validate named-measurement and classification invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_measurement_name(&self.name, "EngineMeasurementV1 name")?;
        validate_measurement_name(&self.unit, "EngineMeasurementV1 unit")?;
        match (self.classification, self.value) {
            (EngineValueClassificationV1::Unavailable, None)
            | (
                EngineValueClassificationV1::Measured | EngineValueClassificationV1::Estimated,
                Some(_),
            ) => Ok(()),
            (EngineValueClassificationV1::Unavailable, Some(_)) => Err(ValidationError::new(
                "EngineMeasurementV1 unavailable values must be omitted",
            )),
            (
                EngineValueClassificationV1::Measured | EngineValueClassificationV1::Estimated,
                None,
            ) => Err(ValidationError::new(
                "EngineMeasurementV1 measured and estimated values require a number",
            )),
        }
    }
}

/// Stable failure taxonomy for a local Engine operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineFailureCodeV1 {
    PolicyRejected,
    SourceUnavailable,
    SourceIntegrityMismatch,
    ResourceLimit,
    UnsupportedOperation,
    Internal,
}

/// Structured, non-secret failure information with an optional recovery route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineFailureV1 {
    pub code: EngineFailureCodeV1,
    pub retryable_by_host: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_ref: Option<ProtocolReference>,
}

impl EngineFailureV1 {
    /// Validate recovery semantics without prescribing host retries.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.code == EngineFailureCodeV1::PolicyRejected
            && (self.retryable_by_host || self.recovery_ref.is_some())
        {
            return Err(ValidationError::new(
                "EngineFailureV1 policy_rejected must not request a host retry or recovery route",
            ));
        }
        if matches!(
            self.code,
            EngineFailureCodeV1::SourceUnavailable | EngineFailureCodeV1::SourceIntegrityMismatch
        ) && self.recovery_ref.is_none()
        {
            return Err(ValidationError::new(
                "EngineFailureV1 source failures require a recovery_ref",
            ));
        }
        Ok(())
    }
}

/// Terminal status of one Engine operation; it never controls the host loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineObservationStatusV1 {
    Succeeded,
    Degraded,
    Rejected,
    Failed,
}

/// Immutable link from an Engine observation to an integrity-addressed receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineReceiptLinkV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub receipt_id: ReceiptId,
    pub receipt_ref: ProtocolReference,
    pub receipt_digest: Sha256Digest,
    pub invocation_id: EngineInvocationIdV1,
}

impl EngineReceiptLinkV1 {
    /// Validate a receipt link and bind it to the supplied invocation.
    pub fn validate_for(&self, invocation: &EngineInvocationV1) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        if self.invocation_id != invocation.invocation_id {
            return Err(ValidationError::new(
                "EngineReceiptLinkV1 invocation_id must match EngineInvocationV1",
            ));
        }
        Ok(())
    }
}

/// Structured output, measurements, failure state, and optional receipt lineage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineObservationV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub invocation_id: EngineInvocationIdV1,
    pub status: EngineObservationStatusV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_ref: Option<ProtocolReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<Sha256Digest>,
    pub source_lineage: Vec<ProtocolReference>,
    pub measurements: Vec<EngineMeasurementV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<EngineFailureV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_link: Option<EngineReceiptLinkV1>,
}

impl EngineObservationV1 {
    /// Validate this observation independently of an invocation record.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        if self.output_ref.is_some() != self.output_digest.is_some() {
            return Err(ValidationError::new(
                "EngineObservationV1 output_ref and output_digest must be present together",
            ));
        }
        validate_nonempty_unique_refs(&self.source_lineage, "EngineObservationV1 source_lineage")?;
        if self.measurements.len() > MAX_MEASUREMENTS {
            return Err(ValidationError::new(format!(
                "EngineObservationV1 measurements exceeds {MAX_MEASUREMENTS}",
            )));
        }
        for measurement in &self.measurements {
            measurement.validate()?;
        }
        if self
            .measurements
            .iter()
            .enumerate()
            .any(|(index, measurement)| {
                self.measurements[..index]
                    .iter()
                    .any(|existing| existing.name == measurement.name)
            })
        {
            return Err(ValidationError::new(
                "EngineObservationV1 measurements contains duplicate names",
            ));
        }
        if let Some(failure) = &self.failure {
            failure.validate()?;
        }
        match (
            self.status,
            self.output_ref.is_some(),
            self.failure.as_ref(),
        ) {
            (EngineObservationStatusV1::Succeeded, true, None)
            | (EngineObservationStatusV1::Degraded, true, Some(_))
            | (EngineObservationStatusV1::Failed, false, Some(_)) => Ok(()),
            (EngineObservationStatusV1::Rejected, false, Some(failure))
                if failure.code == EngineFailureCodeV1::PolicyRejected =>
            {
                Ok(())
            }
            _ => Err(ValidationError::new(
                "EngineObservationV1 status is inconsistent with output and failure",
            )),
        }
    }

    /// Validate linkage, source lineage, policy admission, and receipt binding.
    pub fn validate_for(&self, invocation: &EngineInvocationV1) -> Result<(), ValidationError> {
        invocation.validate()?;
        self.validate()?;
        if self.invocation_id != invocation.invocation_id {
            return Err(ValidationError::new(
                "EngineObservationV1 invocation_id must match EngineInvocationV1",
            ));
        }
        if self
            .source_lineage
            .iter()
            .any(|reference| !invocation.source_refs.contains(reference))
        {
            return Err(ValidationError::new(
                "EngineObservationV1 source_lineage must be a subset of invocation source_refs",
            ));
        }
        match invocation.policy_admission.decision {
            EnginePolicyDecisionV1::Admitted
                if self.status == EngineObservationStatusV1::Rejected =>
            {
                return Err(ValidationError::new(
                    "EngineObservationV1 admitted invocation cannot be rejected",
                ));
            }
            EnginePolicyDecisionV1::Rejected
                if self.status != EngineObservationStatusV1::Rejected =>
            {
                return Err(ValidationError::new(
                    "EngineObservationV1 rejected invocation must produce a rejected observation",
                ));
            }
            _ => {}
        }
        if let Some(receipt_link) = &self.receipt_link {
            receipt_link.validate_for(invocation)?;
        }
        Ok(())
    }
}

/// Published local Engine interface declaration and supported operation set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineInterfaceV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub interface_version: SemanticVersion,
    pub engine: ResolvedLocalEngineIdentityV1,
    pub supported_operations: Vec<EngineOperationV1>,
}

impl EngineInterfaceV1 {
    /// Validate the declared Engine interface without widening it into an SDK.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        self.engine.validate()?;
        if self.supported_operations.is_empty()
            || self.supported_operations.len() > MAX_SUPPORTED_OPERATIONS
        {
            return Err(ValidationError::new(format!(
                "EngineInterfaceV1 supported_operations must contain 1..={MAX_SUPPORTED_OPERATIONS} entries",
            )));
        }
        if self
            .supported_operations
            .iter()
            .enumerate()
            .any(|(index, operation)| {
                self.supported_operations[..index].iter().any(|existing| {
                    existing.capability_id == operation.capability_id
                        && existing.capability_version == operation.capability_version
                })
            })
        {
            return Err(ValidationError::new(
                "EngineInterfaceV1 supported_operations contains duplicate operations",
            ));
        }
        Ok(())
    }
}

fn validate_nonempty_unique_refs(
    references: &[ProtocolReference],
    field: &str,
) -> Result<(), ValidationError> {
    if references.is_empty() || references.len() > MAX_SOURCE_REFS {
        return Err(ValidationError::new(format!(
            "{field} must contain 1..={MAX_SOURCE_REFS} entries",
        )));
    }
    if references.iter().enumerate().any(|(index, reference)| {
        references[..index]
            .iter()
            .any(|existing| existing == reference)
    }) {
        return Err(ValidationError::new(format!("{field} contains duplicates")));
    }
    Ok(())
}

fn validate_measurement_name(value: &str, field: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > MAX_MEASUREMENT_NAME_LENGTH
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        return Err(ValidationError::new(format!(
            "{field} must contain lowercase ASCII letters, digits, '_' or '-' and fit {MAX_MEASUREMENT_NAME_LENGTH} bytes",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const GOLDEN_INVOCATION: &str = r#"{
        "schema_version":1,
        "invocation_id":"engine-invocation-1",
        "engine":{"engine_id":"lean-ctx-local","engine_version":"1.0.0"},
        "operation":{"capability_id":"source-read","capability_version":"1.0.0"},
        "input_ref":"file:src/lib.rs",
        "input_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "source_refs":["file:src/lib.rs","index:project-v1"],
        "policy_admission":{"policy_ref":"policy:local-default","decision":"admitted"}
    }"#;

    const GOLDEN_OBSERVATION: &str = r#"{
        "schema_version":1,
        "invocation_id":"engine-invocation-1",
        "status":"succeeded",
        "output_ref":"view:source-read-1",
        "output_digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "source_lineage":["file:src/lib.rs"],
        "measurements":[
            {"name":"input_bytes","unit":"byte","classification":"measured","value":420},
            {"name":"saved_tokens","unit":"token","classification":"unavailable"}
        ],
        "receipt_link":{
            "schema_version":1,
            "receipt_id":"receipt-engine-1",
            "receipt_ref":"receipt:engine-1",
            "receipt_digest":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "invocation_id":"engine-invocation-1"
        }
    }"#;

    const GOLDEN_INTERFACE: &str = r#"{
        "schema_version":1,
        "interface_version":"1.0.0",
        "engine":{"engine_id":"lean-ctx-local","engine_version":"1.0.0"},
        "supported_operations":[
            {"capability_id":"source-read","capability_version":"1.0.0"}
        ]
    }"#;

    fn invocation() -> EngineInvocationV1 {
        serde_json::from_str(GOLDEN_INVOCATION).expect("golden invocation must decode")
    }

    fn observation() -> EngineObservationV1 {
        serde_json::from_str(GOLDEN_OBSERVATION).expect("golden observation must decode")
    }

    fn interface() -> EngineInterfaceV1 {
        serde_json::from_str(GOLDEN_INTERFACE).expect("golden interface must decode")
    }

    #[test]
    fn golden_records_round_trip_and_bind_lineage() {
        let interface = interface();
        let invocation = invocation();
        let observation = observation();
        interface.validate().expect("golden interface is valid");
        invocation.validate().expect("golden invocation is valid");
        observation
            .validate_for(&invocation)
            .expect("golden observation binds the invocation");
        assert_eq!(
            serde_json::to_value(&interface).expect("serialize interface"),
            serde_json::from_str::<Value>(GOLDEN_INTERFACE).expect("parse golden value")
        );
        assert_eq!(
            serde_json::to_value(&invocation).expect("serialize invocation"),
            serde_json::from_str::<Value>(GOLDEN_INVOCATION).expect("parse golden value")
        );
        assert_eq!(
            serde_json::to_value(&observation).expect("serialize observation"),
            serde_json::from_str::<Value>(GOLDEN_OBSERVATION).expect("parse golden value")
        );
    }

    #[test]
    fn strict_deserialization_rejects_unknown_fields_and_bad_digests() {
        let mut unknown: Value = serde_json::from_str(GOLDEN_INVOCATION).expect("golden JSON");
        unknown["cloud_tenant"] = json!("forbidden");
        assert!(serde_json::from_value::<EngineInvocationV1>(unknown).is_err());

        let mut bad_digest: Value = serde_json::from_str(GOLDEN_INVOCATION).expect("golden JSON");
        bad_digest["input_digest"] = json!("sha256:ABC");
        assert!(serde_json::from_value::<EngineInvocationV1>(bad_digest).is_err());
    }

    #[test]
    fn mutation_rejects_missing_lineage_and_invalid_measurement_state() {
        let mut no_input_ref = invocation();
        no_input_ref.source_refs = vec![ProtocolReference::try_from("index:project-v1").unwrap()];
        assert!(no_input_ref.validate().is_err());

        let mut invalid_measurement = observation();
        invalid_measurement.measurements[1].value = Some(1);
        assert!(invalid_measurement.validate().is_err());
    }

    #[test]
    fn mutation_rejects_policy_and_receipt_cross_bindings() {
        let mut rejected_invocation = invocation();
        rejected_invocation.policy_admission.decision = EnginePolicyDecisionV1::Rejected;
        let successful_observation = observation();
        assert!(
            successful_observation
                .validate_for(&rejected_invocation)
                .is_err()
        );

        let invocation = invocation();
        let mut observation = observation();
        observation.receipt_link.as_mut().unwrap().invocation_id =
            EngineInvocationIdV1::try_from("other-invocation").unwrap();
        assert!(observation.validate_for(&invocation).is_err());
    }

    #[test]
    fn interface_rejects_duplicate_operations_and_unlabelled_cloud_surface() {
        let mut duplicate = interface();
        duplicate
            .supported_operations
            .push(duplicate.supported_operations[0].clone());
        assert!(duplicate.validate().is_err());

        let mut cloud: Value = serde_json::from_str(GOLDEN_INTERFACE).expect("golden JSON");
        cloud["tenant_id"] = json!("forbidden");
        assert!(serde_json::from_value::<EngineInterfaceV1>(cloud).is_err());
    }
}
