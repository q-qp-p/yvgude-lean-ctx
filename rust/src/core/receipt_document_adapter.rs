//! Validated pure join between admitted task/plan and local Engine lineage.

use std::collections::BTreeSet;
use std::fmt;

use lean_ctx_protocol::{
    EngineInvocationV1, EngineObservationV1, EngineReceiptLinkV1, ExecutionPlanV1,
    ProtocolReference, ReceiptCapabilityLinkV1, ReceiptLineageV1, Sha256Digest, TaskEnvelopeV1,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::core::canonical;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReceiptDocumentInputsV1 {
    pub lineage: ReceiptLineageV1,
    pub receipt_link: EngineReceiptLinkV1,
    pub source_lineage: Vec<ProtocolReference>,
    pub input_ref: ProtocolReference,
    pub input_digest: Sha256Digest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReceiptDocumentAdapterError(String);

impl fmt::Display for ReceiptDocumentAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ReceiptDocumentAdapterError {}

/// Validate every cross-document identity before exposing receipt inputs.
pub(crate) fn join_receipt_document_inputs(
    task: &TaskEnvelopeV1,
    plan: &ExecutionPlanV1,
    invocation: &EngineInvocationV1,
    observation: &EngineObservationV1,
) -> Result<ReceiptDocumentInputsV1, ReceiptDocumentAdapterError> {
    task.validate().map_err(protocol_error)?;
    plan.validate().map_err(protocol_error)?;
    observation
        .validate_for(invocation)
        .map_err(protocol_error)?;
    if plan.task_id != task.task_id {
        return fail("execution plan task_id does not match task envelope");
    }
    if !plan
        .capability_ids
        .contains(&invocation.operation.capability_id)
    {
        return fail("execution plan does not admit the invoked capability");
    }
    if plan
        .policy_decision_ref
        .as_deref()
        .is_some_and(|reference| reference != invocation.policy_admission.policy_ref.as_str())
    {
        return fail("execution plan policy decision disagrees with Engine admission");
    }
    let invocation_sources = invocation
        .source_refs
        .iter()
        .map(ProtocolReference::as_str)
        .collect::<BTreeSet<_>>();
    let observation_sources = observation
        .source_lineage
        .iter()
        .map(ProtocolReference::as_str)
        .collect::<BTreeSet<_>>();
    if invocation_sources != observation_sources {
        return fail("Engine observation source lineage is not the exact invocation lineage");
    }
    let receipt_link = observation.receipt_link.as_ref().ok_or_else(|| {
        ReceiptDocumentAdapterError("Engine observation has no receipt link".into())
    })?;
    if !receipt_link
        .receipt_ref
        .as_str()
        .ends_with(receipt_link.receipt_digest.as_str())
    {
        return fail("Engine receipt ref does not bind its advertised digest");
    }

    let invocation_ref = canonical_digest(invocation);
    let lineage = ReceiptLineageV1 {
        task_id: task.task_id.clone(),
        task_ref: canonical_digest(task),
        plan_id: plan.plan_id.clone(),
        plan_ref: canonical_digest(plan),
        invocation_id: invocation.invocation_id.as_str().to_owned(),
        invocation_ref: invocation_ref.clone(),
        identity_ref: canonical_digest(&task.agent_id),
        policy_refs: vec![canonical_digest(&invocation.policy_admission)],
        capabilities: vec![ReceiptCapabilityLinkV1 {
            capability_id: invocation.operation.capability_id.clone(),
            capability_version: invocation.operation.capability_version.clone(),
            invocation_ref,
        }],
    };
    Ok(ReceiptDocumentInputsV1 {
        lineage,
        receipt_link: receipt_link.clone(),
        source_lineage: observation.source_lineage.clone(),
        input_ref: invocation.input_ref.clone(),
        input_digest: invocation.input_digest.clone(),
    })
}

fn canonical_digest<T: Serialize>(value: &T) -> Sha256Digest {
    let mut digest = String::with_capacity(71);
    digest.push_str("sha256:");
    for byte in Sha256::digest(canonical::canonical_serialize(value)) {
        use std::fmt::Write as _;
        write!(&mut digest, "{byte:02x}").expect("writing to String cannot fail");
    }
    Sha256Digest::new(digest).expect("locally generated digest is canonical")
}

fn protocol_error(error: impl fmt::Display) -> ReceiptDocumentAdapterError {
    ReceiptDocumentAdapterError(error.to_string())
}

fn fail<T>(message: &str) -> Result<T, ReceiptDocumentAdapterError> {
    Err(ReceiptDocumentAdapterError(message.to_owned()))
}
