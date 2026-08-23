//! Production composition of the protocol Engine spine into a canonical receipt.

use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey};
use lean_ctx_protocol::{
    AcceptanceState, ContextBalanceV1, EngineInvocationV1, EngineObservationV1, ExecutionPlanV1,
    ReceiptChainLinkV1, ReceiptDocumentV1, ReceiptEvidenceRefV1, ReceiptKeyAdmissionV1,
    ReceiptOutcomeLinkV1, ReceiptSignerV1, ReceiptTerminalStatusV1, ReceiptValueV1, Sha256Digest,
    TaskEnvelopeV1, UtcTimestamp,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    ExecutionEvent, ExecutionLedgerError, ExecutionLedgerStore, PublishedCanonicalReceipt, Result,
    publish_canonical_receipt,
};
use crate::core::receipt_document_adapter::join_receipt_document_inputs;

/// Authoritative terminal observations supplied by the host that owns the task outcome.
#[derive(Debug, Clone)]
pub struct CanonicalReceiptRecordV1 {
    pub context_balance: ContextBalanceV1,
    pub status: ReceiptTerminalStatusV1,
    pub values: Vec<ReceiptValueV1>,
    pub outcome: ReceiptOutcomeLinkV1,
    pub evidence_refs: Vec<ReceiptEvidenceRefV1>,
    pub chain: ReceiptChainLinkV1,
    pub issued_at: UtcTimestamp,
    pub signer_admission: ReceiptSignerAdmissionV1,
}

/// Server-owned trust snapshot authorizing one external receipt-signing key.
#[derive(Debug, Clone)]
pub struct ReceiptSignerAdmissionV1 {
    pub key_id: String,
    pub public_key_digest: Sha256Digest,
    pub admitted_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub revoked_at: Option<UtcTimestamp>,
}

/// Join, sign, durably publish, and ledger-project one real Engine observation.
pub fn record_canonical_engine_receipt(
    task: &TaskEnvelopeV1,
    plan: &ExecutionPlanV1,
    invocation: &EngineInvocationV1,
    observation: &EngineObservationV1,
    record: CanonicalReceiptRecordV1,
    signing_key: &SigningKey,
    ledger: &ExecutionLedgerStore,
) -> Result<PublishedCanonicalReceipt> {
    task.validate().map_err(invalid_protocol)?;
    plan.validate().map_err(invalid_protocol)?;
    record
        .context_balance
        .validate()
        .map_err(invalid_protocol)?;
    validate_terminal_join(record.status, record.outcome.state)?;
    validate_signer_admission(&record.signer_admission, signing_key, &record.issued_at)?;
    invocation.validate().map_err(invalid_protocol)?;
    observation
        .validate_for(invocation)
        .map_err(invalid_protocol)?;
    let inputs = join_receipt_document_inputs(task, plan, invocation, observation)
        .map_err(|error| ExecutionLedgerError::InvalidRecord(error.to_string()))?;

    if !record
        .evidence_refs
        .iter()
        .any(|evidence| evidence.digest == inputs.receipt_link.receipt_digest)
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "canonical receipt evidence must bind the Engine receipt ref and digest".to_owned(),
        ));
    }

    persist_lineage_sidecar(&task, &inputs.lineage.task_ref)?;
    persist_lineage_sidecar(&plan, &inputs.lineage.plan_ref)?;
    persist_lineage_sidecar(&invocation, &inputs.lineage.invocation_ref)?;
    persist_lineage_sidecar(&task.agent_id, &inputs.lineage.identity_ref)?;
    persist_lineage_sidecar(
        &invocation.policy_admission,
        inputs
            .lineage
            .policy_refs
            .first()
            .expect("adapter emits one policy reference"),
    )?;

    let mut receipt = ReceiptDocumentV1 {
        schema_version: 1,
        receipt_id: zero_digest(),
        lineage: inputs.lineage,
        chain: record.chain,
        status: record.status,
        values: record.values,
        outcome: record.outcome,
        evidence_refs: record.evidence_refs,
        issued_at: record.issued_at,
        signer: ReceiptSignerV1 {
            algorithm: "ed25519".to_owned(),
            key_id: record.signer_admission.key_id,
            key_admission: ReceiptKeyAdmissionV1::ExternalTrustStore,
        },
        signature: STANDARD.encode([0_u8; 64]),
    };
    receipt.receipt_id = receipt.derived_receipt_id().map_err(invalid_protocol)?;
    receipt.signature = STANDARD.encode(
        signing_key
            .sign(&receipt.signing_bytes().map_err(invalid_protocol)?)
            .to_bytes(),
    );
    receipt.validate().map_err(invalid_protocol)?;

    let task_id = task.task_id.as_str().to_owned();
    let trace_id = task.trace_id.as_str().to_owned();
    let timestamp = receipt.issued_at.as_str().to_owned();
    ledger.append(ExecutionEvent::TaskStarted {
        task_id: task_id.clone(),
        trace_id: trace_id.clone(),
        envelope_ref: receipt.lineage.task_ref.as_str().to_owned(),
        timestamp: task.created_at.as_str().to_owned(),
        sequence_number: 0,
        prev_hash: String::new(),
    })?;
    ledger.append(ExecutionEvent::PlanCreated {
        task_id: task_id.clone(),
        trace_id: trace_id.clone(),
        plan_id: plan.plan_id.as_str().to_owned(),
        plan_ref: receipt.lineage.plan_ref.as_str().to_owned(),
        timestamp: timestamp.clone(),
        sequence_number: 0,
        prev_hash: String::new(),
    })?;
    ledger.append(ExecutionEvent::ContextDelivered {
        task_id: task_id.clone(),
        trace_id: trace_id.clone(),
        context_balance: record.context_balance,
        timestamp: timestamp.clone(),
        sequence_number: 0,
        prev_hash: String::new(),
    })?;
    ledger.append(ExecutionEvent::EngineInvoked {
        task_id: task_id.clone(),
        trace_id: trace_id.clone(),
        plan_id: plan.plan_id.as_str().to_owned(),
        invocation_id: invocation.invocation_id.as_str().to_owned(),
        invocation_ref: receipt.lineage.invocation_ref.as_str().to_owned(),
        capability_id: invocation.operation.capability_id.as_str().to_owned(),
        capability_version: invocation.operation.capability_version.as_str().to_owned(),
        timestamp: timestamp.clone(),
        sequence_number: 0,
        prev_hash: String::new(),
    })?;

    let published = publish_canonical_receipt(&receipt, ledger, &trace_id, &receipt.issued_at)?;
    if receipt.outcome.state != AcceptanceState::Unknown {
        ledger.append(ExecutionEvent::OutcomeRecorded {
            task_id,
            trace_id,
            outcome_id: receipt
                .outcome
                .outcome_id
                .as_ref()
                .expect("validated terminal outcome has an ID")
                .as_str()
                .to_owned(),
            receipt_id: receipt.receipt_id.as_str().to_owned(),
            accepted: receipt.outcome.state,
            timestamp,
            sequence_number: 0,
            prev_hash: String::new(),
        })?;
    }
    Ok(published)
}

fn persist_lineage_sidecar<T: Serialize>(value: &T, expected: &Sha256Digest) -> Result<()> {
    let bytes = crate::core::canonical::canonical_serialize(value);
    let actual = digest(&bytes);
    if actual != expected.as_str() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "canonical lineage sidecar digest disagrees with adapter join".to_owned(),
        ));
    }
    let digest_hex = actual
        .strip_prefix("sha256:")
        .expect("locally generated digest has prefix");
    drop(
        crate::core::engine_interface::persist_engine_artifact_content(
            "execution/evidence",
            digest_hex,
            "json",
            &bytes,
        )
        .map_err(ExecutionLedgerError::InvalidRecord)?,
    );
    Ok(())
}

fn validate_signer_admission(
    admission: &ReceiptSignerAdmissionV1,
    signing_key: &SigningKey,
    issued_at: &UtcTimestamp,
) -> Result<()> {
    if admission.key_id.is_empty()
        || digest(signing_key.verifying_key().as_bytes()) != admission.public_key_digest.as_str()
        || issued_at.as_str() < admission.admitted_at.as_str()
        || issued_at.as_str() >= admission.expires_at.as_str()
        || admission
            .revoked_at
            .as_ref()
            .is_some_and(|revoked| issued_at.as_str() >= revoked.as_str())
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "receipt signing key is not admitted by the server trust snapshot".to_owned(),
        ));
    }
    Ok(())
}

fn validate_terminal_join(status: ReceiptTerminalStatusV1, outcome: AcceptanceState) -> Result<()> {
    match (status, outcome) {
        (ReceiptTerminalStatusV1::Rejected, AcceptanceState::Rejected)
        | (ReceiptTerminalStatusV1::Succeeded, AcceptanceState::Accepted) => Ok(()),
        (ReceiptTerminalStatusV1::Rejected, _) => Err(ExecutionLedgerError::InvalidRecord(
            "rejected terminal status requires a rejected outcome".to_owned(),
        )),
        (_, AcceptanceState::Rejected) => Err(ExecutionLedgerError::InvalidRecord(
            "rejected outcome requires a rejected terminal status".to_owned(),
        )),
        (_, AcceptanceState::Accepted) => Err(ExecutionLedgerError::InvalidRecord(
            "accepted outcome requires a succeeded terminal status".to_owned(),
        )),
        (_, AcceptanceState::Unknown) => Ok(()),
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(71);
    value.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

fn zero_digest() -> Sha256Digest {
    Sha256Digest::new(format!("sha256:{}", "0".repeat(64))).expect("zero digest is canonical")
}

fn invalid_protocol(error: impl std::fmt::Display) -> ExecutionLedgerError {
    ExecutionLedgerError::InvalidRecord(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lean_ctx_protocol::{
        EnginePolicyAdmissionV1, EnginePolicyDecisionV1, ReceiptEvidenceKindV1,
        ReceiptValueClassificationV1, SignatureStatus,
    };

    #[test]
    fn native_engine_to_signed_receipt_and_ledger_is_one_verified_path() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("fixture.md");
        std::fs::write(&source, "stable native context").unwrap();
        let policy = EnginePolicyAdmissionV1 {
            policy_ref: lean_ctx_protocol::ProtocolReference::new("policy:fixture").unwrap(),
            decision: EnginePolicyDecisionV1::Admitted,
        };
        let engine = crate::core::engine_interface::NativeContextEngine::with_root(root.path());
        let (invocation, observation) = engine
            .execute_ctx_read_snapshot(source.to_str().unwrap(), "stable native context", policy)
            .unwrap();
        let task: TaskEnvelopeV1 = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "task_id": "task-1",
            "trace_id": "trace-1",
            "project_id": "project-1",
            "session_id": "session-1",
            "agent_id": "agent-1",
            "complexity": "medium",
            "created_at": "2026-08-23T11:59:00Z"
        }))
        .unwrap();
        let plan: ExecutionPlanV1 = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "plan_id": "plan-1",
            "task_id": "task-1",
            "context_budget_tokens": 1000,
            "context_strategy": "balanced",
            "knowledge_refs": [],
            "capability_ids": [invocation.operation.capability_id.as_str()],
            "model": "local-engine",
            "provider": "leanctx",
            "reasoning_allocation_milli": 0,
            "max_retries": 0,
            "fallback_refs": [],
            "stop_condition": "on_completion",
            "expected_cost_micros": 0,
            "expected_quality_milli": 900,
            "expected_latency_ms": 100,
            "policy_decision_ref": "policy:fixture"
        }))
        .unwrap();
        let engine_receipt = observation.receipt_link.as_ref().unwrap();
        let evidence = ReceiptEvidenceRefV1 {
            kind: ReceiptEvidenceKindV1::Measurement,
            uri: lean_ctx_protocol::ProtocolReference::new("artifact://engine/receipt").unwrap(),
            digest: engine_receipt.receipt_digest.clone(),
            media_type: "application/json".to_owned(),
            signature_status: SignatureStatus::NotSigned,
        };
        let record = CanonicalReceiptRecordV1 {
            context_balance: ContextBalanceV1 {
                original_tokens: 100,
                materialized_tokens: 80,
                delivered_tokens: 60,
                provider_billed_tokens: 60,
            },
            status: ReceiptTerminalStatusV1::Succeeded,
            values: vec![ReceiptValueV1 {
                name: "input_tokens".to_owned(),
                unit: "token".to_owned(),
                classification: ReceiptValueClassificationV1::Measured,
                value: Some(60),
                evidence_digests: vec![engine_receipt.receipt_digest.clone()],
                formula_digest: None,
                price_table_digest: None,
                reconciliation_digest: None,
            }],
            outcome: ReceiptOutcomeLinkV1 {
                state: AcceptanceState::Unknown,
                outcome_id: None,
                outcome_ref: None,
                acceptance_evidence_digest: None,
            },
            evidence_refs: vec![evidence],
            chain: ReceiptChainLinkV1 {
                chain_id: "chain-1".to_owned(),
                sequence_number: 1,
                previous_receipt_id: None,
                previous_signature_digest: None,
            },
            issued_at: UtcTimestamp::new("2026-08-23T12:00:00Z").unwrap(),
            signer_admission: ReceiptSignerAdmissionV1 {
                key_id: "test-key".to_owned(),
                public_key_digest: Sha256Digest::new(digest(
                    SigningKey::from_bytes(&[17; 32]).verifying_key().as_bytes(),
                ))
                .unwrap(),
                admitted_at: UtcTimestamp::new("2026-01-01T00:00:00Z").unwrap(),
                expires_at: UtcTimestamp::new("2027-01-01T00:00:00Z").unwrap(),
                revoked_at: None,
            },
        };
        let ledger = ExecutionLedgerStore::new(root.path().join("ledger.jsonl"));
        let signing_key = SigningKey::from_bytes(&[17; 32]);
        let mut invalid_terminal = record.clone();
        invalid_terminal.status = ReceiptTerminalStatusV1::Rejected;
        assert!(
            record_canonical_engine_receipt(
                &task,
                &plan,
                &invocation,
                &observation,
                invalid_terminal,
                &signing_key,
                &ledger,
            )
            .is_err()
        );
        assert!(ledger.load().unwrap().is_empty());
        let mut revoked = record.clone();
        revoked.signer_admission.revoked_at =
            Some(UtcTimestamp::new("2026-08-23T11:00:00Z").unwrap());
        assert!(
            record_canonical_engine_receipt(
                &task,
                &plan,
                &invocation,
                &observation,
                revoked,
                &signing_key,
                &ledger,
            )
            .is_err()
        );
        assert!(ledger.load().unwrap().is_empty());
        let published = record_canonical_engine_receipt(
            &task,
            &plan,
            &invocation,
            &observation,
            record.clone(),
            &signing_key,
            &ledger,
        )
        .unwrap();
        let repeated = record_canonical_engine_receipt(
            &task,
            &plan,
            &invocation,
            &observation,
            record,
            &signing_key,
            &ledger,
        )
        .unwrap();

        assert_eq!(published, repeated);
        assert_eq!(ledger.load_verified().unwrap().len(), 5);
        assert!(ledger.verify_chain().unwrap());
        let projected = ledger.canonical_receipt_for_task("task-1").unwrap();
        assert_eq!(projected.receipt_id, published.receipt_id);
        let receipt =
            ReceiptDocumentV1::from_canonical_bytes(&std::fs::read(&published.path).unwrap())
                .unwrap();
        assert_eq!(
            receipt.lineage.invocation_id,
            invocation.invocation_id.as_str()
        );
    }
}
