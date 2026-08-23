//! Read-only projections over execution-ledger events.

use lean_ctx_protocol::{
    EvidenceKind, EvidenceRefV1, ExecutionReceiptV1, ReceiptId, SignatureStatus, TaskId,
};

use super::event::ExecutionEvent;
use super::store::ExecutionLedgerStore;

/// Token and model-call totals for one task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskCostSummary {
    pub task_id: String,
    pub total_tokens: u64,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub cost_micros: u64,
    pub total_cost_micros: u64,
    pub model_calls: u32,
    pub total_latency_ms: u64,
}

/// Verified ledger projection of the authoritative canonical receipt artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalReceiptProjectionV1 {
    pub task_id: String,
    pub invocation_id: String,
    pub receipt_id: String,
    pub receipt_ref: String,
    pub receipt_digest: String,
    pub chain_id: String,
    pub sequence_number: u64,
    pub previous_receipt_id: Option<String>,
}

/// Read-only projection facade for callers that want to keep the store explicit.
pub struct ExecutionProjection<'a> {
    store: &'a ExecutionLedgerStore,
}

impl<'a> ExecutionProjection<'a> {
    #[must_use]
    pub const fn new(store: &'a ExecutionLedgerStore) -> Self {
        Self { store }
    }

    #[must_use]
    pub fn task_cost_summary(&self, task_id: &str) -> TaskCostSummary {
        task_cost_summary_from_store(self.store, task_id)
    }

    #[must_use]
    pub fn receipt_for_task(&self, task_id: &str) -> Option<ExecutionReceiptV1> {
        receipt_for_task_from_store(self.store, task_id)
    }

    /// Returns the latest authoritative canonical receipt for a task.
    #[must_use]
    pub fn canonical_receipt_for_task(
        &self,
        task_id: &str,
    ) -> Option<CanonicalReceiptProjectionV1> {
        canonical_receipt_for_task_from_store(self.store, task_id)
    }
}

impl ExecutionLedgerStore {
    /// Projects token/cost/model-call totals for one task.
    #[must_use]
    pub fn task_cost_summary(&self, task_id: &str) -> TaskCostSummary {
        task_cost_summary_from_store(self, task_id)
    }

    /// Builds the protocol receipt projection for one task.
    #[must_use]
    pub fn receipt_for_task(&self, task_id: &str) -> Option<ExecutionReceiptV1> {
        receipt_for_task_from_store(self, task_id)
    }

    /// Returns the latest authoritative canonical receipt for a task.
    #[must_use]
    pub fn canonical_receipt_for_task(
        &self,
        task_id: &str,
    ) -> Option<CanonicalReceiptProjectionV1> {
        canonical_receipt_for_task_from_store(self, task_id)
    }
}

/// Projects the latest canonical receipt metadata from a verified event snapshot.
#[must_use]
pub fn canonical_receipt_for_task_from_store(
    store: &ExecutionLedgerStore,
    task_id: &str,
) -> Option<CanonicalReceiptProjectionV1> {
    store.by_task(task_id).into_iter().rev().find_map(|event| {
        if let ExecutionEvent::CanonicalReceiptRecorded {
            task_id,
            invocation_id,
            receipt_id,
            receipt_ref,
            receipt_digest,
            receipt_chain_id,
            receipt_sequence_number,
            previous_receipt_id,
            ..
        } = event
        {
            Some(CanonicalReceiptProjectionV1 {
                task_id,
                invocation_id,
                receipt_id,
                receipt_ref,
                receipt_digest,
                chain_id: receipt_chain_id,
                sequence_number: receipt_sequence_number,
                previous_receipt_id,
            })
        } else {
            None
        }
    })
}

/// Projects the default execution ledger for `task_id`.
#[must_use]
pub fn task_cost_summary(task_id: &str) -> TaskCostSummary {
    ExecutionLedgerStore::from_default().map_or_else(
        |_| TaskCostSummary::default(),
        |store| task_cost_summary_from_store(&store, task_id),
    )
}

/// Projects an explicitly selected execution ledger for `task_id`.
#[must_use]
pub fn task_cost_summary_from_store(
    store: &ExecutionLedgerStore,
    task_id: &str,
) -> TaskCostSummary {
    let mut summary = TaskCostSummary {
        task_id: task_id.to_owned(),
        ..TaskCostSummary::default()
    };

    for event in store.by_task(task_id) {
        let ExecutionEvent::ModelInvoked {
            model,
            tokens_in,
            tokens_out,
            latency_ms,
            ..
        } = event
        else {
            continue;
        };
        summary.total_tokens_in = summary.total_tokens_in.saturating_add(tokens_in);
        summary.total_tokens_out = summary.total_tokens_out.saturating_add(tokens_out);
        summary.total_tokens = summary
            .total_tokens_in
            .saturating_add(summary.total_tokens_out);
        summary.model_calls = summary.model_calls.saturating_add(1);
        summary.total_latency_ms = summary.total_latency_ms.saturating_add(latency_ms);
        let cost = estimate_cost_micros(&model, tokens_in, tokens_out);
        summary.cost_micros = summary.cost_micros.saturating_add(cost);
        summary.total_cost_micros = summary.cost_micros;
    }

    summary
}

/// Builds a protocol `ExecutionReceiptV1` from the task's observed events.
///
/// A receipt is emitted only when the event stream contains the observations that
/// make it auditable: at least one model invocation, a context balance, and a
/// signed-receipt event.  Pricing is an estimate from the existing model-pricing
/// table because `ModelInvoked` records usage, while the signed receipt remains
/// the authoritative artifact for provider billing.
#[must_use]
pub fn receipt_for_task(task_id: &str) -> Option<ExecutionReceiptV1> {
    ExecutionLedgerStore::from_default()
        .ok()
        .and_then(|store| receipt_for_task_from_store(&store, task_id))
}

/// Builds a protocol receipt from an explicitly selected store.
#[must_use]
pub fn receipt_for_task_from_store(
    store: &ExecutionLedgerStore,
    task_id: &str,
) -> Option<ExecutionReceiptV1> {
    let events = store.by_task(task_id);
    let task_id = TaskId::try_from(task_id.to_owned()).ok()?;

    let mut plan_id = None;
    let mut context_balance = None;
    let mut receipt_identity: Option<(String, String, String)> = None;
    let mut outcome_ref = None;
    let mut decision_refs = Vec::new();
    let mut model_calls = 0_u32;
    let mut total_input = 0_u64;
    let mut total_output = 0_u64;
    let mut total_latency = 0_u64;
    let mut first_model = None;
    let mut last_model = None;
    let mut provider = None;

    for event in events {
        match event {
            ExecutionEvent::PlanCreated { plan_id: value, .. } => plan_id = Some(value),
            ExecutionEvent::ContextDelivered {
                context_balance: value,
                ..
            } => context_balance = Some(value),
            ExecutionEvent::ModelInvoked {
                plan_id: invoked_plan,
                model,
                provider: invoked_provider,
                tokens_in,
                tokens_out,
                latency_ms,
                ..
            } => {
                if plan_id.is_none() {
                    plan_id = Some(invoked_plan);
                }
                model_calls = model_calls.saturating_add(1);
                total_input = total_input.saturating_add(tokens_in);
                total_output = total_output.saturating_add(tokens_out);
                total_latency = total_latency.saturating_add(latency_ms);
                if first_model.is_none() {
                    first_model = Some(model.clone());
                }
                last_model = Some(model);
                provider = Some(invoked_provider);
            }
            ExecutionEvent::ReceiptSigned {
                receipt_id,
                receipt_hash,
                signature,
                ..
            } => receipt_identity = Some((receipt_id, receipt_hash, signature)),
            ExecutionEvent::OutcomeRecorded { outcome_id, .. } => outcome_ref = Some(outcome_id),
            ExecutionEvent::DecisionRecorded { decision_id, .. } => decision_refs.push(decision_id),
            ExecutionEvent::TaskStarted { .. }
            | ExecutionEvent::EngineInvoked { .. }
            | ExecutionEvent::CanonicalReceiptRecorded { .. } => {}
        }
    }

    let plan_id = plan_id?;
    let plan_id = lean_ctx_protocol::PlanId::try_from(plan_id).ok()?;
    let context_balance = context_balance?;
    let (receipt_id, receipt_hash, signature) = receipt_identity?;
    let receipt_id = ReceiptId::try_from(receipt_id).ok()?;
    let requested_model = first_model?;
    let selected_model = last_model?;
    let provider = provider?;
    if model_calls == 0 {
        return None;
    }

    let actual_cost_micros = estimate_cost_micros(&selected_model, total_input, total_output);
    let evidence_refs = vec![EvidenceRefV1 {
        kind: EvidenceKind::ProviderReceipt,
        uri: format!("receipt:{receipt_hash}"),
        digest: receipt_hash,
        signature_status: if signature.is_empty() {
            SignatureStatus::NotSigned
        } else {
            SignatureStatus::Unverified
        },
    }];

    let receipt = ExecutionReceiptV1 {
        schema_version: 1,
        receipt_id,
        task_id,
        plan_id,
        context_balance,
        fresh_input_tokens: total_input,
        cached_input_tokens: 0,
        output_tokens: total_output,
        reasoning_tokens: 0,
        requested_model,
        selected_model,
        provider,
        capability_id: None,
        capability_version: None,
        model_calls,
        retries: 0,
        latency_ms: total_latency,
        actual_cost_micros,
        baseline_cost_micros: actual_cost_micros,
        avoided_cost_micros: 0,
        etpao_milli: 0,
        outcome_ref,
        knowledge_refs: Vec::new(),
        decision_refs,
        evidence_refs,
        signature,
    };
    receipt.validate().ok()?;
    Some(receipt)
}

fn estimate_cost_micros(model: &str, tokens_in: u64, tokens_out: u64) -> u64 {
    let quote = crate::core::gain::model_pricing::ModelPricing::load().quote(Some(model));
    let usd = quote.cost.estimate_usd(tokens_in, tokens_out, 0, 0);
    if !usd.is_finite() || usd <= 0.0 {
        return 0;
    }
    (usd * 1_000_000.0).round() as u64
}
