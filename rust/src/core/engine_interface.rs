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
const RECOVERY_DIRECTORY: &str = "engine-interface/v1/recovery";

/// Inputs intentionally retained inside the local Engine proof boundary.
///
/// The caller provides source and input identities before dispatch. The bridge
/// verifies the expected input digest against the bytes read through the native
/// rooted adapter, so an observation can never claim a different source input.
#[derive(Clone, Debug)]
struct NativeContextEngineRequest {
    invocation_id: EngineInvocationIdV1,
    input_ref: ProtocolReference,
    input_digest: Sha256Digest,
    source_refs: Vec<ProtocolReference>,
    policy_admission: EnginePolicyAdmissionV1,
    paths: Vec<String>,
    mode: String,
    budget_tokens: Option<u64>,
    timeout_ms: u64,
}

impl NativeContextEngineRequest {
    fn ctx_read_snapshot(
        path: &str,
        raw_input: &str,
        timeout_ms: u64,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(Self, String), String> {
        let input = crate::core::redaction::redact_text_if_enabled(raw_input);
        let raw_input_digest = sha256_digest(raw_input.as_bytes())?;
        let input_digest = sha256_digest(input.as_bytes())?;
        let input_ref = ProtocolReference::new(format!(
            "input:ctx-read-snapshot-sha256:{}",
            raw_input_digest.hex()
        ))
        .map_err(|error| error.to_string())?;
        let canonical_path = std::fs::canonicalize(path)
            .map_err(|error| format!("resolve ctx_read Engine source: {error}"))?;
        let canonical_path = canonical_path.to_string_lossy().into_owned();
        let path_digest = sha256_digest(canonical_path.as_bytes())?;
        let source_ref = ProtocolReference::new(format!(
            "source:canonical-path-sha256:{}",
            path_digest.hex()
        ))
        .map_err(|error| error.to_string())?;
        let invocation_seed = canonical::canonical_serialize(&CtxReadInvocationIdentityV1 {
            engine_id: ENGINE_ID,
            engine_version: env!("CARGO_PKG_VERSION"),
            capability_id: CAPABILITY_ID,
            capability_version: CAPABILITY_VERSION,
            input_ref: input_ref.as_str(),
            source_ref: source_ref.as_str(),
            input_digest: input_digest.as_str(),
            policy_ref: policy_admission.policy_ref.as_str(),
            policy_decision: policy_admission.decision,
            mode: "aggressive",
            timeout_ms,
        });
        let invocation_digest = sha256_digest(&invocation_seed)?;
        let request = Self {
            invocation_id: EngineInvocationIdV1::new(format!(
                "engine-invocation-{}",
                &invocation_digest.hex()[..32]
            ))
            .map_err(|error| error.to_string())?,
            input_ref: input_ref.clone(),
            input_digest,
            source_refs: vec![input_ref, source_ref],
            policy_admission,
            paths: vec![canonical_path],
            mode: "aggressive".to_owned(),
            budget_tokens: None,
            timeout_ms,
        };
        Ok((request, input))
    }
}

#[derive(Serialize)]
struct CtxReadInvocationIdentityV1<'a> {
    engine_id: &'a str,
    engine_version: &'a str,
    capability_id: &'a str,
    capability_version: &'a str,
    input_ref: &'a str,
    source_ref: &'a str,
    input_digest: &'a str,
    policy_ref: &'a str,
    policy_decision: EnginePolicyDecisionV1,
    mode: &'a str,
    timeout_ms: u64,
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

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct EngineRecoveryArtifactV1 {
    schema_version: u32,
    invocation: EngineInvocationV1,
    observation: EngineObservationV1,
    failure_class: &'static str,
}

impl NativeContextEngine {
    #[must_use]
    pub(crate) fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            adapter: NativeContextAdapter::with_root(root),
        }
    }

    fn interface(&self) -> Result<EngineInterfaceV1, String> {
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
    fn execute(
        &self,
        request: NativeContextEngineRequest,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        self.execute_inner(request, None)
    }

    /// Execute against a caller-owned immutable source snapshot.
    ///
    /// The digest is still verified by the adapter; the only difference from
    /// `execute` is that no second disk read can race the production caller.
    fn execute_materialized(
        &self,
        request: NativeContextEngineRequest,
        input: &str,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        self.execute_inner(request, Some(input))
    }

    /// Bind and execute the production `ctx_read` snapshot as one operation.
    pub(crate) fn execute_ctx_read_snapshot(
        &self,
        path: &str,
        raw_input: &str,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        let rooted_path = crate::core::pathjail::jail_path(Path::new(path), self.adapter.root())
            .map_err(|_| "ctx_read Engine source is outside its rooted boundary".to_owned())?;
        let rooted_path = rooted_path.to_string_lossy().into_owned();
        let (request, input) = NativeContextEngineRequest::ctx_read_snapshot(
            &rooted_path,
            raw_input,
            30_000,
            policy_admission,
        )?;
        self.execute_materialized(request, &input)
    }

    pub(crate) fn execute_ctx_read_rejection(
        &self,
        path: &str,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        if policy_admission.decision != EnginePolicyDecisionV1::Rejected {
            return Err("ctx_read rejection requires a rejected policy admission".to_owned());
        }
        let rooted_path = crate::core::pathjail::jail_path(Path::new(path), self.adapter.root())
            .map_err(|_| "ctx_read Engine source is outside its rooted boundary".to_owned())?;
        let (request, _) = NativeContextEngineRequest::ctx_read_snapshot(
            &rooted_path.to_string_lossy(),
            "",
            30_000,
            policy_admission,
        )?;
        self.execute(request)
    }

    fn execute_inner(
        &self,
        request: NativeContextEngineRequest,
        materialized_input: Option<&str>,
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

        let native_result = match materialized_input {
            Some(input) => self
                .adapter
                .invoke_materialized_bounded(native_invocation, input.to_owned()),
            None => self.adapter.invoke_with_output_identity(&native_invocation),
        };
        let observation = match native_result {
            Ok(result) if result.input_digest == request.input_digest.as_str() => {
                let output_digest = Sha256Digest::new(result.output_digest)
                    .map_err(|error| format!("native output digest is invalid: {error}"))?;
                let output_ref = ProtocolReference::new(format!("output:{}", output_digest.hex()))
                    .map_err(|error| error.to_string())?;
                match persist_output(output_digest.hex(), &result.output) {
                    Ok(()) => EngineObservationV1 {
                        schema_version: V1_SCHEMA_VERSION,
                        invocation_id: invocation.invocation_id.clone(),
                        status: EngineObservationStatusV1::Succeeded,
                        output_ref: Some(output_ref),
                        output_digest: Some(output_digest),
                        source_lineage: invocation.source_refs.clone(),
                        measurements: measured_observations(&result.result),
                        failure: None,
                        receipt_link: None,
                    },
                    Err(error) => {
                        tracing::warn!(%error, "failed to persist Engine output artifact");
                        storage_failure_observation(&invocation)
                    }
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
        let receipt_link = match persist_receipt(&invocation, &observation) {
            Ok(receipt_link) => receipt_link,
            Err(error) => {
                let recovery_ref = persist_recovery(
                    &invocation,
                    &observation,
                    "receipt-persistence-failed",
                )
                .map_err(|recovery_error| {
                    format!(
                        "persist Engine receipt: {error}; persist Engine recovery artifact: {recovery_error}"
                    )
                })?;
                return Err(format!(
                    "persist Engine receipt: {error}; recovery_ref={}",
                    recovery_ref.as_str()
                ));
            }
        };
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

fn storage_failure_observation(invocation: &EngineInvocationV1) -> EngineObservationV1 {
    let mut observation = failed_observation(
        invocation,
        EngineFailureCodeV1::Internal,
        Some(invocation.input_ref.clone()),
    );
    if let Some(failure) = observation.failure.as_mut() {
        failure.retryable_by_host = true;
    }
    observation
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

fn persist_recovery(
    invocation: &EngineInvocationV1,
    observation: &EngineObservationV1,
    failure_class: &'static str,
) -> Result<ProtocolReference, String> {
    let artifact = EngineRecoveryArtifactV1 {
        schema_version: V1_SCHEMA_VERSION,
        invocation: invocation.clone(),
        observation: observation.clone(),
        failure_class,
    };
    let bytes = canonical::canonical_serialize(&artifact);
    let digest = sha256_digest(&bytes)?;
    persist_content(RECOVERY_DIRECTORY, digest.hex(), "json", &bytes)?;
    ProtocolReference::new(format!("recovery:sha256:{}", digest.hex()))
        .map_err(|error| error.to_string())
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
        if let Some(permissions) = artifact_permissions() {
            std::fs::set_permissions(&path, permissions)
                .map_err(|error| format!("harden Engine artifact permissions: {error}"))?;
        }
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
    fn rejected_receipt_matches_the_versioned_golden_fixture() {
        let input_ref = ProtocolReference::new("input:fixture").expect("input ref");
        let source_ref = ProtocolReference::new("source:fixture").expect("source ref");
        let invocation = EngineInvocationV1 {
            schema_version: V1_SCHEMA_VERSION,
            invocation_id: EngineInvocationIdV1::new("engine-invocation-fixture-v1")
                .expect("invocation id"),
            engine: ResolvedLocalEngineIdentityV1 {
                engine_id: ENGINE_ID.to_owned(),
                engine_version: SemanticVersion::new("1.0.0").expect("engine version"),
            },
            operation: native_operation().expect("operation"),
            input_ref: input_ref.clone(),
            input_digest: Sha256Digest::new(format!("sha256:{}", "0".repeat(64)))
                .expect("input digest"),
            source_refs: vec![input_ref, source_ref],
            policy_admission: EnginePolicyAdmissionV1 {
                policy_ref: ProtocolReference::new("policy:fixture").expect("policy ref"),
                decision: EnginePolicyDecisionV1::Rejected,
            },
        };
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
        let artifact = EngineReceiptArtifactV1 {
            schema_version: V1_SCHEMA_VERSION,
            invocation,
            observation,
        };
        let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/contracts/engine-interface/v1/rejected-receipt.json"
        )))
        .expect("golden receipt fixture");

        assert_eq!(
            canonical::canonical_serialize(&artifact),
            canonical::canonical_serialize(&fixture)
        );
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
        assert_eq!(first.receipt_link, second.receipt_link);
    }

    #[test]
    fn materialized_execution_binds_receipt_to_the_callers_exact_snapshot() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        std::fs::write(root.path().join("fixture.md"), "different disk bytes")
            .expect("fixture write");
        std::fs::create_dir(root.path().join("alias-parent")).expect("alias directory");
        let input = "caller snapshot\nwith stable context";
        let engine = NativeContextEngine::with_root(root.path());
        let policy_ref = "policy:ctx-read-context-gate-v1:fixture";
        let policy_admission = EnginePolicyAdmissionV1 {
            policy_ref: ProtocolReference::new(policy_ref).expect("policy ref"),
            decision: EnginePolicyDecisionV1::Admitted,
        };
        let source_path = root.path().join("fixture.md");
        let (request, prepared_input) = NativeContextEngineRequest::ctx_read_snapshot(
            &source_path.to_string_lossy(),
            input,
            30_000,
            policy_admission.clone(),
        )
        .expect("production request");
        let (alias_request, alias_prepared_input) = NativeContextEngineRequest::ctx_read_snapshot(
            &root
                .path()
                .join("alias-parent/../fixture.md")
                .to_string_lossy(),
            input,
            30_000,
            policy_admission,
        )
        .expect("canonical alias request");
        assert_eq!(request.invocation_id, alias_request.invocation_id);
        assert_eq!(request.source_refs, alias_request.source_refs);
        assert_eq!(prepared_input, alias_prepared_input);
        assert!(
            request
                .input_ref
                .as_str()
                .starts_with("input:ctx-read-snapshot-sha256:")
        );

        let (invocation, observation) = engine
            .execute_materialized(request, &prepared_input)
            .expect("materialized Engine invocation");

        assert_eq!(invocation.input_digest, digest(prepared_input.as_bytes()));
        assert_eq!(invocation.policy_admission.policy_ref.as_str(), policy_ref);
        assert_eq!(observation.status, EngineObservationStatusV1::Succeeded);
        assert!(observation.receipt_link.is_some());
        assert!(
            observation
                .measurements
                .iter()
                .all(|measurement| measurement.name != "latency_ms")
        );
    }

    #[test]
    fn production_snapshot_refuses_a_source_outside_the_engine_root() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let outside = tempfile::tempdir().expect("outside root");
        let source = outside.path().join("escape.md");
        std::fs::write(&source, "outside").expect("outside fixture");
        let engine = NativeContextEngine::with_root(root.path());
        let admission = EnginePolicyAdmissionV1 {
            policy_ref: ProtocolReference::new("policy:ctx-read-context-gate-v1:fixture")
                .expect("policy ref"),
            decision: EnginePolicyDecisionV1::Admitted,
        };

        let error = engine
            .execute_ctx_read_snapshot(&source.to_string_lossy(), "outside", admission)
            .unwrap_err();
        assert_eq!(
            error,
            "ctx_read Engine source is outside its rooted boundary"
        );
    }

    #[test]
    fn materialized_execution_enforces_a_real_host_deadline() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let source = root.path().join("fixture.md");
        std::fs::write(&source, "deadline fixture").expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path());
        let admission = EnginePolicyAdmissionV1 {
            policy_ref: ProtocolReference::new("policy:ctx-read-context-gate-v1:fixture")
                .expect("policy ref"),
            decision: EnginePolicyDecisionV1::Admitted,
        };
        let (request, input) = NativeContextEngineRequest::ctx_read_snapshot(
            &source.to_string_lossy(),
            "deadline fixture",
            0,
            admission,
        )
        .expect("bounded request");

        let (_, observation) = engine
            .execute_materialized(request, &input)
            .expect("deadline failure receipt");
        assert_eq!(observation.status, EngineObservationStatusV1::Failed);
        assert_eq!(
            observation.failure.expect("deadline failure").code,
            EngineFailureCodeV1::ResourceLimit
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_engine_artifact_permissions_are_rehardened() {
        use std::os::unix::fs::PermissionsExt;

        let _data_dir = data_dir::isolated_data_dir();
        let bytes = b"permission fixture";
        let digest = digest(bytes);
        persist_output(digest.hex(), bytes).expect("initial artifact");
        let path = data_dir::lean_ctx_data_dir()
            .expect("isolated data dir")
            .join(OUTPUT_DIRECTORY)
            .join(format!("{}.txt", digest.hex()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("loosen fixture permissions");

        persist_output(digest.hex(), bytes).expect("reharden existing artifact");
        assert_eq!(
            std::fs::metadata(path)
                .expect("artifact metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_engine_artifact_rejects_tampering_and_symlinks() {
        use std::os::unix::fs::symlink;

        let _data_dir = data_dir::isolated_data_dir();
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let output_dir = data_dir.join(OUTPUT_DIRECTORY);
        std::fs::create_dir_all(&output_dir).expect("output directory");
        let bytes = b"expected output";
        let digest = digest(bytes);
        let path = output_dir.join(format!("{}.txt", digest.hex()));
        std::fs::write(&path, b"tampered output").expect("tampered artifact");
        assert!(
            persist_output(digest.hex(), bytes)
                .unwrap_err()
                .contains("digest")
        );

        std::fs::remove_file(&path).expect("remove tampered artifact");
        let target = output_dir.join("target.txt");
        std::fs::write(&target, bytes).expect("symlink target");
        symlink(&target, &path).expect("artifact symlink");
        assert!(
            persist_output(digest.hex(), bytes)
                .unwrap_err()
                .contains("regular non-symlink")
        );
    }

    #[test]
    fn ctx_read_identity_is_bound_to_raw_snapshot_bytes() {
        let root = tempfile::tempdir().expect("native adapter root");
        let path = root.path().join("fixture.md");
        std::fs::write(&path, "fixture").expect("fixture write");
        let admission = EnginePolicyAdmissionV1 {
            policy_ref: ProtocolReference::new("policy:ctx-read-context-gate-v1:fixture")
                .expect("policy ref"),
            decision: EnginePolicyDecisionV1::Admitted,
        };
        let (first, _) = NativeContextEngineRequest::ctx_read_snapshot(
            &path.to_string_lossy(),
            "raw snapshot A",
            30_000,
            admission.clone(),
        )
        .expect("first request");
        let (second, _) = NativeContextEngineRequest::ctx_read_snapshot(
            &path.to_string_lossy(),
            "raw snapshot B",
            30_000,
            admission,
        )
        .expect("second request");

        assert_ne!(first.input_ref, second.input_ref);
        assert_ne!(first.invocation_id, second.invocation_id);
    }

    #[test]
    fn production_snapshot_redacts_secret_before_output_and_recovery_persistence() {
        let _data_dir = data_dir::isolated_data_dir();
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let engine_dir = data_dir.join("engine-interface/v1");
        std::fs::create_dir_all(&engine_dir).expect("Engine directory");
        std::fs::write(engine_dir.join("receipts"), "blocks receipt directory")
            .expect("receipt blocker");
        let root = tempfile::tempdir().expect("native adapter root");
        let path = root.path().join("secret.md");
        std::fs::write(&path, "source placeholder").expect("fixture write");
        let secret = format!(
            "api_key={}",
            ["not", "-a-real-secret-", "1234567890abcdef"].concat()
        );
        let admission = EnginePolicyAdmissionV1 {
            policy_ref: ProtocolReference::new("policy:ctx-read-context-gate-v1:redaction")
                .expect("policy ref"),
            decision: EnginePolicyDecisionV1::Admitted,
        };

        let error = NativeContextEngine::with_root(root.path())
            .execute_ctx_read_snapshot(&path.to_string_lossy(), &secret, admission)
            .expect_err("blocked receipt must return a durable recovery error");
        assert!(!error.contains(&secret));

        for directory in [OUTPUT_DIRECTORY, RECOVERY_DIRECTORY] {
            for entry in std::fs::read_dir(data_dir.join(directory)).expect("artifact directory") {
                let bytes =
                    std::fs::read(entry.expect("artifact entry").path()).expect("artifact bytes");
                assert!(!String::from_utf8_lossy(&bytes).contains(&secret));
            }
        }
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
    fn output_persistence_failure_is_receipted_and_retryable() {
        let _data_dir = data_dir::isolated_data_dir();
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let engine_dir = data_dir.join("engine-interface/v1");
        std::fs::create_dir_all(&engine_dir).expect("Engine directory");
        std::fs::write(engine_dir.join("outputs"), "blocks output directory")
            .expect("output blocker");
        let root = tempfile::tempdir().expect("native adapter root");
        let input = b"retryable native context";
        std::fs::write(root.path().join("fixture.md"), input).expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path());
        let request = request("fixture.md", input, EnginePolicyDecisionV1::Admitted);

        let (_, failed) = engine.execute(request.clone()).expect("failed receipt");
        assert_eq!(failed.status, EngineObservationStatusV1::Failed);
        assert!(failed.receipt_link.is_some());
        let failure = failed.failure.expect("failure record");
        assert_eq!(failure.code, EngineFailureCodeV1::Internal);
        assert!(failure.retryable_by_host);

        std::fs::remove_file(engine_dir.join("outputs")).expect("remove output blocker");
        std::fs::create_dir(engine_dir.join("outputs")).expect("output directory");
        let (_, retried) = engine.execute(request).expect("successful retry");
        assert_eq!(retried.status, EngineObservationStatusV1::Succeeded);
        assert!(retried.receipt_link.is_some());
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
