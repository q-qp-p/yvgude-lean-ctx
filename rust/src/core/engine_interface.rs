//! Internal proof bridge from one native OCLA capability to Engine Interface v1.
//!
//! This module is deliberately crate-private. It proves the local Engine
//! contract without promoting an SDK façade, an agent loop, or Cloud semantics.

use std::path::Path;

use lean_ctx_protocol::{
    CapabilityId, EngineFailureCodeV1, EngineFailureV1, EngineInterfaceV1, EngineInvocationIdV1,
    EngineInvocationV1, EngineMeasurementV1, EngineObservationStatusV1, EngineObservationV1,
    EngineOperationV1, EnginePolicyAdmissionV1, EnginePolicyDecisionV1, EngineReceiptLinkV1,
    EngineValueClassificationV1, ProtocolReference, ReceiptId, ResolvedLocalEngineIdentityV1,
    SemanticVersion, Sha256Digest, V1_SCHEMA_VERSION,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::core::atomic_fs;
use crate::core::canonical;
use crate::core::data_dir;
use crate::core::ocla::adapters::native_context::{
    NativeContextAdapter, NativeContextInvocationFailure,
};
use crate::core::ocla::invocation::{CapabilityInput, CapabilityInvocation, PolicyConstraints};

const ENGINE_ID: &str = "lean-ctx-local";
const CAPABILITY_ID: &str = "capability://leanctx/context-optimization";
const CAPABILITY_VERSION: &str = "1.0.0";
const RECEIPT_DIRECTORY: &str = "engine-interface/v1/receipts";
const OUTPUT_DIRECTORY: &str = "engine-interface/v1/outputs";

/// Inputs intentionally retained inside the local Engine proof boundary.
///
/// The caller provides source and input identities before dispatch. The bridge
/// verifies the expected input digest against the bytes read through the native
/// rooted adapter, so an observation can never claim a different source input.
#[derive(Clone, Debug)]
pub(crate) struct NativeContextEngineRequest {
    pub invocation_id: EngineInvocationIdV1,
    pub input_ref: ProtocolReference,
    pub input_digest: Sha256Digest,
    pub source_refs: Vec<ProtocolReference>,
    pub policy_admission: EnginePolicyAdmissionV1,
    pub paths: Vec<String>,
    pub mode: String,
    pub budget_tokens: Option<u64>,
    pub timeout_ms: u64,
}

/// One strictly local implementation of the published Engine records.
pub(crate) struct NativeContextEngine {
    adapter: NativeContextAdapter,
}

/// Stored without a receipt link so its digest cannot self-reference.
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct EngineReceiptArtifactV1 {
    schema_version: u32,
    invocation: EngineInvocationV1,
    observation: EngineObservationV1,
}

impl NativeContextEngine {
    #[must_use]
    pub(crate) fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            adapter: NativeContextAdapter::with_root(root),
        }
    }

    pub(crate) fn interface(&self) -> Result<EngineInterfaceV1, String> {
        let interface = EngineInterfaceV1 {
            schema_version: V1_SCHEMA_VERSION,
            interface_version: SemanticVersion::new("1.0.0").map_err(|error| error.to_string())?,
            engine: engine_identity()?,
            supported_operations: vec![native_operation()?],
        };
        interface.validate().map_err(|error| error.to_string())?;
        Ok(interface)
    }

    /// Execute the one admitted local capability and return its factual record.
    ///
    /// Rejections never enter the adapter. Successful output is persisted only
    /// as a local integrity-addressed artifact; the observation exposes its
    /// reference and SHA-256, never the payload.
    pub(crate) fn execute(
        &self,
        request: NativeContextEngineRequest,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        let invocation = self.invocation_for(&request)?;
        if request.policy_admission.decision == EnginePolicyDecisionV1::Rejected {
            let observation = EngineObservationV1 {
                schema_version: V1_SCHEMA_VERSION,
                invocation_id: invocation.invocation_id.clone(),
                status: EngineObservationStatusV1::Rejected,
                output_ref: None,
                output_digest: None,
                source_lineage: invocation.source_refs.clone(),
                measurements: Vec::new(),
                failure: Some(EngineFailureV1 {
                    code: EngineFailureCodeV1::PolicyRejected,
                    retryable_by_host: false,
                    recovery_ref: None,
                }),
                receipt_link: None,
            };
            return self.persist_terminal(invocation, observation);
        }

        let native_invocation = CapabilityInvocation {
            task_id: request.invocation_id.as_str().to_owned(),
            capability_id: CAPABILITY_ID.to_owned(),
            capability_version: CAPABILITY_VERSION.to_owned(),
            input: CapabilityInput::ContextRequest {
                paths: request.paths,
                mode: request.mode,
                budget_tokens: request.budget_tokens,
            },
            policy_constraints: PolicyConstraints::default(),
            timeout_ms: request.timeout_ms,
        };

        let observation = match self.adapter.invoke_with_output_identity(&native_invocation) {
            Ok(result) if result.input_digest == request.input_digest.as_str() => {
                let output_digest = Sha256Digest::new(result.output_digest)
                    .map_err(|error| format!("native output digest is invalid: {error}"))?;
                let output_ref = ProtocolReference::new(format!("output:{}", output_digest.hex()))
                    .map_err(|error| error.to_string())?;
                persist_output(output_digest.hex(), &result.output)?;
                EngineObservationV1 {
                    schema_version: V1_SCHEMA_VERSION,
                    invocation_id: invocation.invocation_id.clone(),
                    status: EngineObservationStatusV1::Succeeded,
                    output_ref: Some(output_ref),
                    output_digest: Some(output_digest),
                    source_lineage: invocation.source_refs.clone(),
                    measurements: measured_observations(&result.result),
                    failure: None,
                    receipt_link: None,
                }
            }
            Ok(_) => source_integrity_failure(&invocation),
            Err(failure) => adapter_failure(&invocation, &failure),
        };
        self.persist_terminal(invocation, observation)
    }

    fn invocation_for(
        &self,
        request: &NativeContextEngineRequest,
    ) -> Result<EngineInvocationV1, String> {
        let invocation = EngineInvocationV1 {
            schema_version: V1_SCHEMA_VERSION,
            invocation_id: request.invocation_id.clone(),
            engine: engine_identity()?,
            operation: native_operation()?,
            input_ref: request.input_ref.clone(),
            input_digest: request.input_digest.clone(),
            source_refs: request.source_refs.clone(),
            policy_admission: request.policy_admission.clone(),
        };
        invocation.validate().map_err(|error| error.to_string())?;
        Ok(invocation)
    }

    fn persist_terminal(
        &self,
        invocation: EngineInvocationV1,
        observation: EngineObservationV1,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        observation
            .validate_for(&invocation)
            .map_err(|error| format!("invalid Engine observation: {error}"))?;
        let receipt_link = persist_receipt(&invocation, &observation)?;
        let mut observation = observation;
        observation.receipt_link = Some(receipt_link);
        observation
            .validate_for(&invocation)
            .map_err(|error| format!("invalid Engine observation receipt link: {error}"))?;
        Ok((invocation, observation))
    }
}

fn engine_identity() -> Result<ResolvedLocalEngineIdentityV1, String> {
    let identity = ResolvedLocalEngineIdentityV1 {
        engine_id: ENGINE_ID.to_owned(),
        engine_version: SemanticVersion::new(env!("CARGO_PKG_VERSION"))
            .map_err(|error| error.to_string())?,
    };
    identity.validate().map_err(|error| error.to_string())?;
    Ok(identity)
}

fn native_operation() -> Result<EngineOperationV1, String> {
    Ok(EngineOperationV1 {
        capability_id: CapabilityId::new(CAPABILITY_ID).map_err(|error| error.to_string())?,
        capability_version: SemanticVersion::new(CAPABILITY_VERSION)
            .map_err(|error| error.to_string())?,
    })
}

fn measured_observations(
    result: &crate::core::ocla::invocation::CapabilityResult,
) -> Vec<EngineMeasurementV1> {
    let mut measurements = vec![
        measured("input_tokens", "token", result.observation.input_tokens),
        measured("output_tokens", "token", result.output_tokens),
        measured("latency_ms", "millisecond", result.latency_ms),
    ];
    for name in ["compression_saved_tokens", "compression_rate_milli"] {
        if let Some(value) = result.observation.metrics.get(name) {
            let unit = if name == "compression_saved_tokens" {
                "token"
            } else {
                "milliunit"
            };
            measurements.push(measured(name, unit, *value));
        }
    }
    measurements
}

fn measured(name: &str, unit: &str, value: u64) -> EngineMeasurementV1 {
    EngineMeasurementV1 {
        name: name.to_owned(),
        unit: unit.to_owned(),
        classification: EngineValueClassificationV1::Measured,
        value: Some(value),
    }
}

fn source_integrity_failure(invocation: &EngineInvocationV1) -> EngineObservationV1 {
    failed_observation(
        invocation,
        EngineFailureCodeV1::SourceIntegrityMismatch,
        Some(invocation.input_ref.clone()),
    )
}

fn adapter_failure(
    invocation: &EngineInvocationV1,
    failure: &NativeContextInvocationFailure,
) -> EngineObservationV1 {
    let code = match failure {
        NativeContextInvocationFailure::SourceUnavailable(_) => {
            EngineFailureCodeV1::SourceUnavailable
        }
        NativeContextInvocationFailure::ResourceLimit(_) => EngineFailureCodeV1::ResourceLimit,
        NativeContextInvocationFailure::UnsupportedInput => {
            EngineFailureCodeV1::UnsupportedOperation
        }
        NativeContextInvocationFailure::InvalidRequest(_) => EngineFailureCodeV1::Internal,
    };
    let recovery_ref = matches!(code, EngineFailureCodeV1::SourceUnavailable)
        .then(|| invocation.input_ref.clone());
    failed_observation(invocation, code, recovery_ref)
}

fn failed_observation(
    invocation: &EngineInvocationV1,
    code: EngineFailureCodeV1,
    recovery_ref: Option<ProtocolReference>,
) -> EngineObservationV1 {
    EngineObservationV1 {
        schema_version: V1_SCHEMA_VERSION,
        invocation_id: invocation.invocation_id.clone(),
        status: EngineObservationStatusV1::Failed,
        output_ref: None,
        output_digest: None,
        source_lineage: invocation.source_refs.clone(),
        measurements: Vec::new(),
        failure: Some(EngineFailureV1 {
            code,
            retryable_by_host: false,
            recovery_ref,
        }),
        receipt_link: None,
    }
}

fn persist_output(digest: &str, bytes: &[u8]) -> Result<(), String> {
    persist_content(OUTPUT_DIRECTORY, digest, "txt", bytes)
}

fn persist_receipt(
    invocation: &EngineInvocationV1,
    observation: &EngineObservationV1,
) -> Result<EngineReceiptLinkV1, String> {
    let artifact = EngineReceiptArtifactV1 {
        schema_version: V1_SCHEMA_VERSION,
        invocation: invocation.clone(),
        observation: observation.clone(),
    };
    let bytes = canonical::canonical_serialize(&artifact);
    let digest = sha256_digest(&bytes)?;
    persist_content(RECEIPT_DIRECTORY, digest.hex(), "json", &bytes)?;
    Ok(EngineReceiptLinkV1 {
        schema_version: V1_SCHEMA_VERSION,
        receipt_id: ReceiptId::new(format!("engine-receipt-{}", &digest.hex()[..32]))
            .map_err(|error| error.to_string())?,
        receipt_ref: ProtocolReference::new(format!("receipt:sha256:{}", digest.hex()))
            .map_err(|error| error.to_string())?,
        receipt_digest: digest,
        invocation_id: invocation.invocation_id.clone(),
    })
}

fn persist_content(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let data_dir = data_dir::lean_ctx_data_dir()?;
    let directory = data_dir.join(directory);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("create Engine artifact directory: {error}"))?;
    data_dir::ensure_dir_permissions(&directory);
    let path = directory.join(format!("{digest}.{extension}"));
    if path.exists() {
        verify_existing_artifact(&path, digest)?;
        return Ok(());
    }

    let permissions = artifact_permissions();
    atomic_fs::write_bytes_with_fallback(&path, bytes, permissions.as_ref())?;
    verify_existing_artifact(&path, digest)
}

fn verify_existing_artifact(path: &Path, digest: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("read Engine artifact metadata: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Engine artifact path must be a regular non-symlink file".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("read Engine artifact: {error}"))?;
    let actual = sha256_digest(&bytes)?;
    if actual.hex() != digest {
        return Err("Engine artifact digest does not match its content-addressed path".to_owned());
    }
    Ok(())
}

fn sha256_digest(bytes: &[u8]) -> Result<Sha256Digest, String> {
    Sha256Digest::new(format!(
        "sha256:{}",
        crate::core::agent_identity::hex_encode(&Sha256::digest(bytes))
    ))
    .map_err(|error| error.to_string())
}

fn artifact_permissions() -> Option<std::fs::Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(value: &[u8]) -> Sha256Digest {
        sha256_digest(value).expect("test SHA-256 digest")
    }

    fn request(
        path: &str,
        input: &[u8],
        decision: EnginePolicyDecisionV1,
    ) -> NativeContextEngineRequest {
        let input_ref = ProtocolReference::new("source:fixture-document").expect("input reference");
        NativeContextEngineRequest {
            invocation_id: EngineInvocationIdV1::new("engine-invocation-fixture")
                .expect("invocation id"),
            input_ref: input_ref.clone(),
            input_digest: digest(input),
            source_refs: vec![
                input_ref,
                ProtocolReference::new("source:fixture-lineage").expect("source reference"),
            ],
            policy_admission: EnginePolicyAdmissionV1 {
                policy_ref: ProtocolReference::new("policy:local-default")
                    .expect("policy reference"),
                decision,
            },
            paths: vec![path.to_owned()],
            mode: "raw".to_owned(),
            budget_tokens: None,
            timeout_ms: 0,
        }
    }

    #[test]
    fn admitted_native_operation_persists_integrity_addressed_output_and_receipt() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let input = b"stable native context";
        std::fs::write(root.path().join("fixture.md"), input).expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path());

        let (invocation, observation) = engine
            .execute(request(
                "fixture.md",
                input,
                EnginePolicyDecisionV1::Admitted,
            ))
            .expect("native Engine invocation");

        assert_eq!(invocation.engine.engine_id, ENGINE_ID);
        assert_eq!(invocation.operation.capability_id.as_str(), CAPABILITY_ID);
        assert_eq!(observation.status, EngineObservationStatusV1::Succeeded);
        observation
            .validate_for(&invocation)
            .expect("Engine observation linkage");
        let output_digest = observation.output_digest.as_ref().expect("output digest");
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let output = data_dir
            .join(OUTPUT_DIRECTORY)
            .join(format!("{}.txt", output_digest.hex()));
        assert_eq!(std::fs::read(&output).expect("stored output"), input);

        let receipt = observation.receipt_link.as_ref().expect("receipt link");
        let receipt_path = data_dir
            .join(RECEIPT_DIRECTORY)
            .join(format!("{}.json", receipt.receipt_digest.hex()));
        let receipt_bytes = std::fs::read(receipt_path).expect("stored receipt");
        assert_eq!(digest(&receipt_bytes), receipt.receipt_digest);
        assert!(!String::from_utf8_lossy(&receipt_bytes).contains("stable native context"));
    }

    #[test]
    fn repeated_native_invocation_keeps_deterministic_identity_and_output() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let input = b"stable native context";
        std::fs::write(root.path().join("fixture.md"), input).expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path());
        let request = request("fixture.md", input, EnginePolicyDecisionV1::Admitted);

        let (first_invocation, first) = engine.execute(request.clone()).expect("first invocation");
        let (second_invocation, second) = engine.execute(request).expect("second invocation");

        assert_eq!(first_invocation.engine, second_invocation.engine);
        assert_eq!(first_invocation.operation, second_invocation.operation);
        assert_eq!(
            first_invocation.input_digest,
            second_invocation.input_digest
        );
        assert_eq!(first_invocation.source_refs, second_invocation.source_refs);
        assert_eq!(first.output_digest, second.output_digest);
    }

    #[test]
    fn rejected_policy_never_attempts_the_missing_source() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let engine = NativeContextEngine::with_root(root.path());

        let (_, observation) = engine
            .execute(request(
                "missing.md",
                b"unread input candidate",
                EnginePolicyDecisionV1::Rejected,
            ))
            .expect("policy rejection record");

        assert_eq!(observation.status, EngineObservationStatusV1::Rejected);
        assert_eq!(
            observation.failure.expect("failure record").code,
            EngineFailureCodeV1::PolicyRejected
        );
    }

    #[test]
    fn missing_source_has_structured_recovery_route() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let engine = NativeContextEngine::with_root(root.path());

        let (_, observation) = engine
            .execute(request(
                "missing.md",
                b"expected source bytes",
                EnginePolicyDecisionV1::Admitted,
            ))
            .expect("source failure record");

        let failure = observation.failure.expect("failure record");
        assert_eq!(failure.code, EngineFailureCodeV1::SourceUnavailable);
        assert!(failure.recovery_ref.is_some());
    }

    #[test]
    fn source_integrity_mismatch_is_explicit() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        std::fs::write(root.path().join("fixture.md"), b"actual source").expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path());

        let (_, observation) = engine
            .execute(request(
                "fixture.md",
                b"different expected source",
                EnginePolicyDecisionV1::Admitted,
            ))
            .expect("integrity failure record");

        let failure = observation.failure.expect("failure record");
        assert_eq!(failure.code, EngineFailureCodeV1::SourceIntegrityMismatch);
        assert!(failure.recovery_ref.is_some());
    }

    #[test]
    fn interface_matches_the_native_capability_contract() {
        let root = tempfile::tempdir().expect("native adapter root");
        let engine = NativeContextEngine::with_root(root.path());
        let interface = engine.interface().expect("Engine interface");
        assert_eq!(interface.engine.engine_id, ENGINE_ID);
        assert_eq!(interface.supported_operations.len(), 1);
        assert_eq!(
            interface.supported_operations[0].capability_id.as_str(),
            CAPABILITY_ID
        );
    }
}
