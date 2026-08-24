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
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::canonical;
#[cfg(test)]
use crate::core::data_dir;
use crate::core::execution_ledger::{ExecutionEvent, ExecutionLedgerStore};
use crate::core::invocation_admission::VerifiedInvocationAdmissionV1;
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

use super::engine_artifact as artifact_store;

pub(super) fn persist_engine_artifact_content(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<std::fs::File, String> {
    artifact_store::persist_content(directory, digest, extension, bytes)
}

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
        let canonical_path = std::fs::canonicalize(path)
            .map_err(|error| format!("resolve ctx_read Engine source: {error}"))?;
        Self::ctx_read_snapshot_canonical(&canonical_path, raw_input, timeout_ms, policy_admission)
    }

    fn ctx_read_snapshot_canonical(
        canonical_path: &Path,
        raw_input: &str,
        timeout_ms: u64,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(Self, String), String> {
        if !canonical_path.is_absolute() {
            return Err("ctx_read Engine canonical source must be absolute".to_owned());
        }
        Self::ctx_read_identified(
            canonical_path.to_string_lossy().into_owned(),
            "source:canonical-path-sha256",
            raw_input,
            timeout_ms,
            policy_admission,
        )
    }

    fn ctx_read_rejection(
        requested_path: &str,
        timeout_ms: u64,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<Self, String> {
        if !Path::new(requested_path).is_absolute() {
            return Err("ctx_read Engine requested source must be absolute".to_owned());
        }
        Self::ctx_read_identified(
            requested_path.to_owned(),
            "source:requested-path-sha256",
            "",
            timeout_ms,
            policy_admission,
        )
        .map(|(request, _)| request)
    }

    fn ctx_read_identified(
        path_identity: String,
        source_prefix: &str,
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
        let path_digest = sha256_digest(path_identity.as_bytes())?;
        let source_ref = ProtocolReference::new(format!("{source_prefix}:{}", path_digest.hex()))
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
            paths: vec![path_identity],
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

fn append_verified_admission(
    ledger: &ExecutionLedgerStore,
    admission: &VerifiedInvocationAdmissionV1,
    trace_id: &str,
) -> Result<bool, String> {
    if trace_id.is_empty() {
        return Err("Engine admission trace_id must be non-empty".to_owned());
    }
    let binding = admission.binding();
    ledger
        .append_if_new(ExecutionEvent::AdmissionConsumed {
            admission_id: binding.admission_id.as_str().to_owned(),
            binding_digest: admission.binding_digest().as_str().to_owned(),
            task_id: binding.task_id.as_str().to_owned(),
            trace_id: trace_id.to_owned(),
            invocation_id: binding.invocation_id.as_str().to_owned(),
            timestamp: binding.issued_at.as_str().to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        })
        .map_err(|error| error.to_string())
}

fn validate_admission_binding(
    binding_invocation_id: &EngineInvocationIdV1,
    binding_invocation_ref: &Sha256Digest,
    invocation: &EngineInvocationV1,
) -> Result<(), String> {
    if binding_invocation_id != &invocation.invocation_id {
        return Err("engine admission invocation_id does not match runtime invocation".to_owned());
    }
    let invocation_ref = sha256_digest(&canonical::canonical_serialize(invocation))?;
    if binding_invocation_ref != &invocation_ref {
        return Err("engine admission invocation_ref does not match runtime invocation".to_owned());
    }
    Ok(())
}

fn dispatch_after_consumption<T, F>(consumed: Result<bool, String>, execute: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    match consumed {
        Ok(true) => execute(),
        Ok(false) => Err("Engine admission was already consumed".to_owned()),
        Err(error) => Err(error),
    }
}

fn dispatch_after_admission<T, F>(
    ledger: &ExecutionLedgerStore,
    admission: &VerifiedInvocationAdmissionV1,
    trace_id: &str,
    execute: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    dispatch_after_consumption(
        append_verified_admission(ledger, admission, trace_id),
        execute,
    )
}

/// Stored without a receipt link so its digest cannot self-reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineReceiptArtifactV1 {
    schema_version: u32,
    invocation: EngineInvocationV1,
    observation: EngineObservationV1,
}

/// Receipt contents returned only after the on-disk artifact and both expected
/// records have passed the local verification boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedEngineReceiptV1 {
    artifact: EngineReceiptArtifactV1,
    digest: Sha256Digest,
}

impl VerifiedEngineReceiptV1 {
    pub(crate) fn invocation(&self) -> &EngineInvocationV1 {
        &self.artifact.invocation
    }

    pub(crate) fn observation(&self) -> &EngineObservationV1 {
        &self.artifact.observation
    }

    pub(crate) fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

/// Build the exact canonical bytes persisted for one Engine receipt.
pub(crate) fn canonical_engine_receipt_artifact_bytes(
    invocation: &EngineInvocationV1,
    observation: &EngineObservationV1,
) -> Vec<u8> {
    canonical::canonical_serialize(&EngineReceiptArtifactV1 {
        schema_version: V1_SCHEMA_VERSION,
        invocation: invocation.clone(),
        observation: observation.clone(),
    })
}

/// Read and verify one Engine receipt artifact against its expected records.
pub(crate) fn read_verified_engine_receipt(
    digest: &Sha256Digest,
    expected_invocation: &EngineInvocationV1,
    expected_observation_without_link: &EngineObservationV1,
) -> Result<VerifiedEngineReceiptV1, String> {
    if expected_observation_without_link.receipt_link.is_some() {
        return Err("expected Engine observation must omit receipt_link".to_owned());
    }
    expected_invocation
        .validate()
        .map_err(|error| format!("invalid expected Engine invocation: {error}"))?;
    expected_observation_without_link
        .validate_for(expected_invocation)
        .map_err(|error| format!("invalid expected Engine observation: {error}"))?;

    let bytes = artifact_store::read_content(RECEIPT_DIRECTORY, digest.hex(), "json")?;
    let actual_digest = sha256_digest(&bytes)?;
    if actual_digest != *digest {
        return Err("Engine receipt artifact digest mismatch".to_owned());
    }
    let artifact = decode_canonical_engine_receipt_artifact(&bytes)?;
    if artifact.schema_version != V1_SCHEMA_VERSION {
        return Err("Engine receipt artifact schema_version must be 1".to_owned());
    }
    if artifact.observation.receipt_link.is_some() {
        return Err("Engine receipt artifact must omit receipt_link".to_owned());
    }
    artifact
        .invocation
        .validate()
        .map_err(|error| format!("invalid Engine receipt invocation: {error}"))?;
    artifact
        .observation
        .validate_for(&artifact.invocation)
        .map_err(|error| format!("invalid Engine receipt observation: {error}"))?;
    if artifact.invocation != *expected_invocation {
        return Err("Engine receipt invocation does not match expectation".to_owned());
    }
    if artifact.observation != *expected_observation_without_link {
        return Err("Engine receipt observation does not match expectation".to_owned());
    }
    Ok(VerifiedEngineReceiptV1 {
        artifact,
        digest: digest.clone(),
    })
}

fn decode_canonical_engine_receipt_artifact(
    bytes: &[u8],
) -> Result<EngineReceiptArtifactV1, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let artifact = EngineReceiptArtifactV1::deserialize(&mut deserializer)
        .map_err(|error| format!("decode Engine receipt artifact: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("trailing Engine receipt artifact data: {error}"))?;
    if canonical_engine_receipt_artifact_bytes(&artifact.invocation, &artifact.observation) != bytes
    {
        return Err("Engine receipt artifact is not canonical JSON".to_owned());
    }
    Ok(artifact)
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
    pub(crate) fn with_root(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = crate::core::pathutil::canonicalize_secure(root.as_ref())
            .map_err(|_| "ctx_read Engine root cannot be bound securely".to_owned())?;
        Ok(Self {
            adapter: NativeContextAdapter::with_root(root),
        })
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
        self.execute_ctx_read_rooted_snapshot(
            &rooted_path.to_string_lossy(),
            raw_input,
            policy_admission,
        )
    }

    pub(crate) fn execute_ctx_read_rooted_snapshot(
        &self,
        rooted_path: &str,
        raw_input: &str,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        let root = self.adapter.root();
        let rooted_path = Path::new(rooted_path);
        if !rooted_path.is_absolute() || !rooted_path.starts_with(root) {
            return Err("ctx_read Engine source is outside its rooted boundary".to_owned());
        }
        let (request, input) = NativeContextEngineRequest::ctx_read_snapshot_canonical(
            rooted_path,
            raw_input,
            30_000,
            policy_admission,
        )?;
        self.execute_materialized(request, &input)
    }

    /// Gate an admitted rooted dispatch on one atomic admission consumption.
    pub(crate) fn execute_ctx_read_rooted_snapshot_admitted(
        &self,
        ledger: &ExecutionLedgerStore,
        admission: &VerifiedInvocationAdmissionV1,
        trace_id: &str,
        rooted_path: &str,
        raw_input: &str,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        let root = self.adapter.root();
        let rooted_path = Path::new(rooted_path);
        if !rooted_path.is_absolute() || !rooted_path.starts_with(root) {
            return Err("ctx_read Engine source is outside its rooted boundary".to_owned());
        }
        let (request, input) = NativeContextEngineRequest::ctx_read_snapshot_canonical(
            rooted_path,
            raw_input,
            30_000,
            policy_admission,
        )?;
        let invocation = self.invocation_for(&request)?;
        let binding = admission.binding();
        validate_admission_binding(&binding.invocation_id, &binding.invocation_ref, &invocation)?;
        dispatch_after_admission(ledger, admission, trace_id, || {
            self.execute_materialized(request, &input)
        })
    }

    pub(crate) fn execute_ctx_read_rejection(
        &self,
        path: &str,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(EngineInvocationV1, EngineObservationV1), String> {
        if policy_admission.decision != EnginePolicyDecisionV1::Rejected {
            return Err("ctx_read rejection requires a rejected policy admission".to_owned());
        }
        let request =
            NativeContextEngineRequest::ctx_read_rejection(path, 30_000, policy_admission)?;
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
    let bytes = canonical_engine_receipt_artifact_bytes(invocation, observation);
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
    artifact_store::persist_content(directory, digest, extension, bytes).map(|_| ())
}

fn sha256_digest(bytes: &[u8]) -> Result<Sha256Digest, String> {
    Sha256Digest::new(format!(
        "sha256:{}",
        crate::core::agent_identity::hex_encode(&Sha256::digest(bytes))
    ))
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_admission_never_dispatches_the_native_adapter() {
        let called = std::cell::Cell::new(false);
        let retry = dispatch_after_consumption(Ok(false), || {
            called.set(true);
            Ok::<_, String>(())
        });
        assert!(retry.is_err());
        assert!(!called.get());

        let error = dispatch_after_consumption(Err("ledger unavailable".to_owned()), || {
            called.set(true);
            Ok::<_, String>(())
        });
        assert!(error.is_err());
        assert!(!called.get());
    }

    fn runtime_invocation_fixture() -> EngineInvocationV1 {
        let root = tempfile::tempdir().expect("native adapter root");
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
        engine
            .invocation_for(&request(
                "fixture.md",
                b"runtime invocation",
                EnginePolicyDecisionV1::Admitted,
            ))
            .expect("runtime invocation")
    }

    fn assert_binding_mismatch_does_not_append_or_dispatch(
        binding_invocation_id: &EngineInvocationIdV1,
        binding_invocation_ref: &Sha256Digest,
        invocation: &EngineInvocationV1,
    ) {
        let directory = tempfile::tempdir().expect("ledger directory");
        let ledger = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        let before = ledger.load().expect("empty ledger");
        let called = std::cell::Cell::new(false);
        let result =
            validate_admission_binding(binding_invocation_id, binding_invocation_ref, invocation)
                .and_then(|()| {
                    dispatch_after_consumption(Ok(true), || {
                        called.set(true);
                        Ok::<_, String>(())
                    })
                });

        assert!(result.is_err());
        assert!(!called.get());
        assert_eq!(ledger.load().expect("ledger after mismatch"), before);
    }

    #[test]
    fn invocation_id_mismatch_is_rejected_before_append_or_adapter() {
        let invocation = runtime_invocation_fixture();
        let invocation_ref = sha256_digest(&canonical::canonical_serialize(&invocation))
            .expect("runtime invocation digest");
        let wrong_id =
            EngineInvocationIdV1::new("engine-invocation-other").expect("mismatched invocation id");

        assert_binding_mismatch_does_not_append_or_dispatch(
            &wrong_id,
            &invocation_ref,
            &invocation,
        );
    }

    #[test]
    fn invocation_ref_mismatch_is_rejected_before_append_or_adapter() {
        let invocation = runtime_invocation_fixture();
        let wrong_ref = Sha256Digest::new(format!("sha256:{}", "0".repeat(64)))
            .expect("mismatched invocation ref");

        assert_binding_mismatch_does_not_append_or_dispatch(
            &invocation.invocation_id,
            &wrong_ref,
            &invocation,
        );
    }

    #[test]
    fn accepted_consumption_dispatches_exactly_once() {
        let called = std::cell::Cell::new(0u8);
        let result = dispatch_after_consumption(Ok(true), || {
            called.set(called.get() + 1);
            Ok::<_, String>(())
        });

        assert!(result.is_ok());
        assert_eq!(called.get(), 1);
    }

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

    fn receipt_fixture(input: &[u8]) -> (EngineInvocationV1, EngineObservationV1, Sha256Digest) {
        let root = tempfile::tempdir().expect("native adapter root");
        std::fs::write(root.path().join("fixture.md"), input).expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
        let (invocation, mut observation) = engine
            .execute(request(
                "fixture.md",
                input,
                EnginePolicyDecisionV1::Admitted,
            ))
            .expect("native Engine invocation");
        let digest = observation
            .receipt_link
            .take()
            .expect("receipt link")
            .receipt_digest;
        (invocation, observation, digest)
    }

    fn persist_raw_receipt(bytes: &[u8]) -> Sha256Digest {
        let digest = digest(bytes);
        persist_engine_artifact_content(RECEIPT_DIRECTORY, digest.hex(), "json", bytes)
            .expect("raw receipt artifact");
        digest
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
    fn verified_engine_receipt_round_trip_returns_bound_token() {
        let _data_dir = data_dir::isolated_data_dir();
        let (invocation, observation, digest) = receipt_fixture(b"verified receipt input");

        let verified = read_verified_engine_receipt(&digest, &invocation, &observation)
            .expect("receipt should verify");

        assert_eq!(verified.invocation(), &invocation);
        assert_eq!(verified.observation(), &observation);
        assert_eq!(verified.digest(), &digest);
    }

    #[test]
    fn verified_engine_receipt_rejects_mixed_records_and_receipt_link_expectations() {
        let _data_dir = data_dir::isolated_data_dir();
        let (first_invocation, _, _) = receipt_fixture(b"first receipt input");
        let (second_invocation, mut second_observation, second_digest) =
            receipt_fixture(b"second receipt input");

        assert_ne!(
            first_invocation.input_digest,
            second_invocation.input_digest
        );
        assert!(
            read_verified_engine_receipt(&second_digest, &first_invocation, &second_observation)
                .is_err(),
            "mixed invocation and observation must fail exact binding"
        );

        second_observation.receipt_link = Some(EngineReceiptLinkV1 {
            schema_version: V1_SCHEMA_VERSION,
            receipt_id: ReceiptId::new("engine-receipt-test-link").expect("receipt id"),
            receipt_ref: ProtocolReference::new("receipt:sha256:test").expect("receipt ref"),
            receipt_digest: second_digest.clone(),
            invocation_id: second_invocation.invocation_id.clone(),
        });
        assert!(
            read_verified_engine_receipt(&second_digest, &second_invocation, &second_observation)
                .is_err(),
            "expected observation must omit receipt_link"
        );
    }

    #[test]
    fn verified_engine_receipt_rejects_noncanonical_duplicate_unknown_and_trailing_json() {
        let _data_dir = data_dir::isolated_data_dir();
        let (invocation, observation, _) = receipt_fixture(b"strict receipt input");
        let canonical = canonical_engine_receipt_artifact_bytes(&invocation, &observation);

        let mut noncanonical = Vec::with_capacity(canonical.len() + 1);
        noncanonical.push(b' ');
        noncanonical.extend_from_slice(&canonical);
        let digest = persist_raw_receipt(&noncanonical);
        assert!(
            read_verified_engine_receipt(&digest, &invocation, &observation).is_err(),
            "leading whitespace must fail canonical-byte equality"
        );

        let canonical_text = String::from_utf8(canonical.clone()).expect("canonical JSON");
        let duplicate = canonical_text.replacen('{', "{\"schema_version\":1,", 1);
        let digest = persist_raw_receipt(duplicate.as_bytes());
        assert!(
            read_verified_engine_receipt(&digest, &invocation, &observation).is_err(),
            "duplicate fields must fail canonical-byte equality"
        );

        let mut unknown = canonical.clone();
        let end = unknown.len() - 1;
        unknown.splice(end..end, b",\"unknown\":true".iter().copied());
        let digest = persist_raw_receipt(&unknown);
        assert!(
            read_verified_engine_receipt(&digest, &invocation, &observation).is_err(),
            "unknown fields must be denied"
        );

        let mut trailing = canonical;
        trailing.extend_from_slice(b"{}");
        let digest = persist_raw_receipt(&trailing);
        assert!(
            read_verified_engine_receipt(&digest, &invocation, &observation).is_err(),
            "trailing JSON values must be denied"
        );
    }

    #[test]
    fn verified_engine_receipt_rejects_tampering_wrong_digest_and_path_prefixes() {
        let _data_dir = data_dir::isolated_data_dir();
        let (invocation, observation, digest) = receipt_fixture(b"tamper-resistant input");
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let receipt_path = data_dir
            .join(RECEIPT_DIRECTORY)
            .join(format!("{}.json", digest.hex()));
        let mut tampered = std::fs::read(&receipt_path).expect("stored receipt");
        tampered[0] = if tampered[0] == b'{' { b'[' } else { b'{' };
        std::fs::write(&receipt_path, &tampered).expect("tamper receipt");
        assert!(
            read_verified_engine_receipt(&digest, &invocation, &observation).is_err(),
            "tampered bytes must fail the requested digest check"
        );

        let wrong_digest =
            Sha256Digest::new(format!("sha256:{}", "0".repeat(64))).expect("wrong digest");
        assert!(
            read_verified_engine_receipt(&wrong_digest, &invocation, &observation).is_err(),
            "wrong digest must not resolve a different artifact"
        );
        assert!(
            artifact_store::read_content(RECEIPT_DIRECTORY, "../receipts", "json").is_err(),
            "path prefixes must not escape the artifact namespace"
        );
    }

    #[cfg(unix)]
    #[test]
    fn verified_engine_receipt_rejects_symlinked_leaf() {
        use std::os::unix::fs::symlink;

        let _data_dir = data_dir::isolated_data_dir();
        let (invocation, observation, digest) = receipt_fixture(b"symlink-resistant input");
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let receipt_path = data_dir
            .join(RECEIPT_DIRECTORY)
            .join(format!("{}.json", digest.hex()));
        let outside = tempfile::NamedTempFile::new().expect("outside artifact");
        std::fs::write(
            outside.path(),
            canonical_engine_receipt_artifact_bytes(&invocation, &observation),
        )
        .expect("outside receipt");
        std::fs::remove_file(&receipt_path).expect("remove receipt");
        symlink(outside.path(), &receipt_path).expect("symlink receipt");

        assert!(
            read_verified_engine_receipt(&digest, &invocation, &observation).is_err(),
            "symlinked receipt leaf must be rejected"
        );
    }

    #[test]
    fn admitted_native_operation_persists_integrity_addressed_output_and_receipt() {
        let _data_dir = data_dir::isolated_data_dir();
        let root = tempfile::tempdir().expect("native adapter root");
        let input = b"stable native context";
        std::fs::write(root.path().join("fixture.md"), input).expect("fixture write");
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");

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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
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
        let control = std::sync::Arc::new(
            crate::core::ocla::adapters::native_context::MaterializedTestControl::new(),
        );
        let engine = NativeContextEngine {
            adapter: NativeContextAdapter::with_root(root.path())
                .with_materialized_test_control(control.clone()),
        };
        let admission = EnginePolicyAdmissionV1 {
            policy_ref: ProtocolReference::new("policy:ctx-read-context-gate-v1:fixture")
                .expect("policy ref"),
            decision: EnginePolicyDecisionV1::Admitted,
        };
        let (request, input) = NativeContextEngineRequest::ctx_read_snapshot(
            &source.to_string_lossy(),
            "deadline fixture",
            25,
            admission,
        )
        .expect("bounded request");

        let started = std::time::Instant::now();
        let (_, observation) = engine
            .execute_materialized(request, &input)
            .expect("deadline failure receipt");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(observation.status, EngineObservationStatusV1::Failed);
        assert_eq!(
            observation.failure.as_ref().expect("deadline failure").code,
            EngineFailureCodeV1::ResourceLimit
        );
        assert!(observation.receipt_link.is_some());
        let output_dir = data_dir::lean_ctx_data_dir()
            .expect("isolated data dir")
            .join(OUTPUT_DIRECTORY);
        assert!(!output_dir.exists());
        control.release.wait();
        control.completed.wait();
        assert!(!output_dir.exists());
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
                .contains("engine_artifact_leaf_untrusted")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_engine_artifact_directory_is_rejected_before_any_write() {
        use std::os::unix::fs::symlink;

        let _data_dir = data_dir::isolated_data_dir();
        let data_dir = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let engine_dir = data_dir.join("engine-interface/v1");
        std::fs::create_dir_all(&engine_dir).expect("Engine directory");
        let outside = tempfile::tempdir().expect("outside directory");
        symlink(outside.path(), engine_dir.join("outputs")).expect("artifact directory symlink");
        let bytes = b"must remain inside the Engine data root";
        let digest = digest(bytes);

        let error = persist_output(digest.hex(), bytes).expect_err("symlinked directory rejected");

        assert!(error.contains("engine_artifact_boundary_rejected"));
        assert_eq!(
            std::fs::read_dir(outside.path())
                .expect("outside directory")
                .count(),
            0
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn descriptor_relative_parent_swap_never_writes_replacement_or_outside() {
        #[cfg(unix)]
        use std::os::unix::fs::symlink;

        let _data_dir = data_dir::isolated_data_dir();
        let data_root = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let output_dir = data_root.join(OUTPUT_DIRECTORY);
        let opened_dir = data_root.join("engine-interface/v1/outputs.opened");
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_path = outside.path().to_path_buf();
        let sentinel = outside_path.join("sentinel.txt");
        std::fs::write(&sentinel, b"OUTSIDE_SENTINEL_V1").expect("outside sentinel");
        let bytes = b"descriptor-relative artifact";
        let digest = digest(bytes);
        let final_name = format!("{}.txt", digest.hex());

        let barrier_output_dir = output_dir.clone();
        let barrier_opened_dir = opened_dir.clone();
        let barrier_outside_path = outside_path.clone();
        let barrier = Box::new(move || {
            std::fs::rename(&barrier_output_dir, &barrier_opened_dir)
                .expect("rename opened directory");
            #[cfg(unix)]
            symlink(&barrier_outside_path, &barrier_output_dir).expect("replacement symlink");
            #[cfg(windows)]
            {
                let _ = barrier_outside_path;
                std::fs::create_dir(&barrier_output_dir).expect("replacement directory");
            }
        });
        let result = artifact_store::persist_content_with_test_barrier(
            OUTPUT_DIRECTORY,
            digest.hex(),
            "txt",
            bytes,
            barrier,
        );

        assert_eq!(
            std::fs::read(&sentinel)
                .expect("outside sentinel remains")
                .as_slice(),
            b"OUTSIDE_SENTINEL_V1"
        );
        assert_eq!(
            std::fs::read_dir(&outside_path)
                .expect("outside directory")
                .count(),
            1,
            "replacement/outside received no artifact or temporary leaf"
        );
        #[cfg(windows)]
        assert_eq!(
            std::fs::read_dir(&output_dir)
                .expect("replacement directory")
                .count(),
            0,
            "replacement directory received no artifact or temporary leaf"
        );
        match result {
            Ok(_) => {
                assert_eq!(
                    std::fs::read(opened_dir.join(final_name)).expect("held directory artifact"),
                    bytes
                );
                assert_eq!(
                    std::fs::read_dir(opened_dir)
                        .expect("held directory")
                        .count(),
                    1,
                    "held directory contains only the published artifact"
                );
            }
            Err(error) => {
                assert!(error.starts_with("engine_artifact_"));
                assert!(!error.contains("errno"));
                assert!(
                    std::fs::read_dir(opened_dir)
                        .expect("held directory")
                        .flatten()
                        .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp")),
                    "failed publication leaves no temporary leaf"
                );
            }
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn descriptor_bound_root_relocation_never_retargets_a_replacement_root() {
        let _data_dir = data_dir::isolated_data_dir();
        let data_root = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let opened_root = data_root.with_extension("opened");
        let bytes = b"descriptor-bound root artifact";
        let digest = digest(bytes);
        let final_name = format!("{}.txt", digest.hex());

        let relocated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let barrier_data_root = data_root.clone();
        let barrier_opened_root = opened_root.clone();
        let barrier_relocated = std::sync::Arc::clone(&relocated);
        let barrier =
            Box::new(
                move || match std::fs::rename(&barrier_data_root, &barrier_opened_root) {
                    Ok(()) => {
                        std::fs::create_dir(&barrier_data_root).expect("replacement data root");
                        barrier_relocated.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(error) => {
                        #[cfg(windows)]
                        assert_eq!(
                            error.kind(),
                            std::io::ErrorKind::PermissionDenied,
                            "Windows may only reject relocation while held handles are open"
                        );
                        #[cfg(unix)]
                        panic!("relocate bound data root: {error}");
                    }
                },
            );
        let result = artifact_store::persist_content_with_test_barrier(
            OUTPUT_DIRECTORY,
            digest.hex(),
            "txt",
            bytes,
            barrier,
        );

        let did_relocate = relocated.load(std::sync::atomic::Ordering::SeqCst);
        let bound_root = if did_relocate {
            assert_eq!(
                std::fs::read_dir(&data_root)
                    .expect("replacement data root")
                    .count(),
                0,
                "replacement root received no artifact or temporary leaf"
            );
            &opened_root
        } else {
            #[cfg(unix)]
            {
                panic!("Unix relocation must succeed");
            }
            #[cfg(windows)]
            {
                assert!(
                    !opened_root.exists(),
                    "denied relocation must not create a partial target"
                );
                &data_root
            }
        };
        match result {
            Ok(_) => assert_eq!(
                std::fs::read(bound_root.join(OUTPUT_DIRECTORY).join(final_name))
                    .expect("artifact remains under bound root object"),
                bytes
            ),
            Err(error) => {
                assert!(error.starts_with("engine_artifact_"));
                assert!(
                    std::fs::read_dir(bound_root.join(OUTPUT_DIRECTORY))
                        .expect("bound output directory")
                        .flatten()
                        .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp")),
                    "failed publication leaves no known temporary leaf"
                );
            }
        }
        if did_relocate {
            std::fs::remove_dir_all(&opened_root).expect("remove relocated test root");
        }
    }

    #[cfg(unix)]
    #[test]
    fn swapped_unix_temp_leaf_is_rejected_and_never_published() {
        use std::os::unix::fs::symlink;

        let _data_dir = data_dir::isolated_data_dir();
        let data_root = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        let output_dir = data_root.join(OUTPUT_DIRECTORY);
        let outside = tempfile::tempdir().expect("outside directory");
        let sentinel = outside.path().join("sentinel.txt");
        std::fs::write(&sentinel, b"OUTSIDE_SENTINEL_V1").expect("outside sentinel");
        let bytes = b"held temporary artifact";
        let digest = digest(bytes);
        let temp_path = output_dir.join(format!(".{}.txt.tmp", digest.hex()));
        let final_path = output_dir.join(format!("{}.txt", digest.hex()));

        let barrier_temp_path = temp_path.clone();
        let barrier_sentinel = sentinel.clone();
        let barrier = Box::new(move || {
            std::fs::remove_file(&barrier_temp_path).expect("unlink held temporary name");
            symlink(&barrier_sentinel, &barrier_temp_path).expect("swap temporary name");
        });
        let error = artifact_store::persist_content_with_test_publish_barrier(
            OUTPUT_DIRECTORY,
            digest.hex(),
            "txt",
            bytes,
            barrier,
        )
        .expect_err("swapped temporary leaf rejected");

        assert_eq!(error, "engine_artifact_leaf_untrusted");
        assert!(!final_path.exists());
        assert!(!temp_path.exists());
        assert_eq!(
            std::fs::read(&sentinel).expect("outside sentinel remains"),
            b"OUTSIDE_SENTINEL_V1"
        );
    }

    #[cfg(windows)]
    #[test]
    fn oversized_windows_artifact_component_is_rejected_before_child_mutation() {
        let _data_dir = data_dir::isolated_data_dir();
        let bytes = b"bounded Windows component";
        let digest = digest(bytes);
        let oversized = "x".repeat((u16::MAX as usize / 2) + 1);

        let error = artifact_store::persist_content(&oversized, digest.hex(), "txt", bytes)
            .expect_err("oversized component rejected");

        assert_eq!(error, "engine_artifact_boundary_rejected");
        let data_root = data_dir::lean_ctx_data_dir().expect("isolated data dir");
        assert_eq!(std::fs::read_dir(data_root).expect("data root").count(), 0);
    }

    #[test]
    fn failed_engine_artifact_publish_leaves_final_absent_and_retryable() {
        let _data_dir = data_dir::isolated_data_dir();
        let bytes = b"failure-atomic artifact fixture";
        let digest = digest(bytes);
        let output_dir = data_dir::lean_ctx_data_dir()
            .expect("isolated data dir")
            .join(OUTPUT_DIRECTORY);
        let final_path = output_dir.join(format!("{}.txt", digest.hex()));

        artifact_store::inject_test_pre_publish_failure();
        assert_eq!(
            persist_output(digest.hex(), bytes).expect_err("injected publish failure"),
            "engine_artifact_test_pre_publish_failure"
        );
        assert!(
            !final_path.exists(),
            "failed publish must not expose final path"
        );
        assert_eq!(
            std::fs::read_dir(&output_dir)
                .expect("output directory")
                .count(),
            0,
            "failed publish must clean its temporary leaf"
        );

        persist_output(digest.hex(), bytes).expect("retry publishes complete artifact");
        assert_eq!(
            std::fs::read(&final_path).expect("published artifact"),
            bytes
        );
        assert_eq!(
            std::fs::read_dir(&output_dir)
                .expect("output directory")
                .count(),
            1,
            "successful retry leaves only the addressed artifact"
        );
    }

    #[cfg(windows)]
    #[test]
    fn failed_windows_temp_validation_cleans_provisional_leaf_and_is_retryable() {
        let _data_dir = data_dir::isolated_data_dir();
        let bytes = b"Windows temp validation fixture";
        let digest = digest(bytes);
        let output_dir = data_dir::lean_ctx_data_dir()
            .expect("isolated data dir")
            .join(OUTPUT_DIRECTORY);
        let final_path = output_dir.join(format!("{}.txt", digest.hex()));

        artifact_store::inject_test_temp_validation_failure();
        assert_eq!(
            persist_output(digest.hex(), bytes).expect_err("injected validation failure"),
            "engine_artifact_leaf_untrusted"
        );
        assert_eq!(
            std::fs::read_dir(&output_dir)
                .expect("output directory")
                .count(),
            0,
            "failed validation must delete its provisional leaf"
        );

        persist_output(digest.hex(), bytes).expect("retry publishes complete artifact");
        assert_eq!(
            std::fs::read(&final_path).expect("published artifact after retry"),
            bytes
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
            .expect("secure Engine root")
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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");

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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");

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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");

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
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
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
    fn engine_root_binding_never_falls_back_to_an_unresolved_path() {
        let parent = tempfile::tempdir().expect("root parent");
        let missing = parent.path().join("missing-root");

        let error = NativeContextEngine::with_root(&missing)
            .err()
            .expect("missing Engine root rejected");

        assert_eq!(error, "ctx_read Engine root cannot be bound securely");
        assert!(!missing.exists());
    }

    #[test]
    fn interface_matches_the_native_capability_contract() {
        let root = tempfile::tempdir().expect("native adapter root");
        let engine = NativeContextEngine::with_root(root.path()).expect("secure Engine root");
        let interface = engine.interface().expect("Engine interface");
        assert_eq!(interface.engine.engine_id, ENGINE_ID);
        assert_eq!(interface.supported_operations.len(), 1);
        assert_eq!(
            interface.supported_operations[0].capability_id.as_str(),
            CAPABILITY_ID
        );
    }
}
