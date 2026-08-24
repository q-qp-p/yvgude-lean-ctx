//! Deterministic execution-ledger hashing and chain verification.

use std::collections::{HashMap, HashSet};

use super::event::ExecutionEvent;
use super::{ExecutionLedgerError, Result};

/// Hash-chain anchor shared with the Savings Ledger.
pub const GENESIS: &str = "genesis";

/// Hashes one event using the Savings Ledger's SHA-256 primitive.
///
/// The canonical event bytes are compact JSON emitted in declaration order.  The
/// preceding hash is also supplied as the hash primitive's domain input, matching
/// `SavingsEvent` chaining semantics.
pub fn hash_event(event: &ExecutionEvent) -> serde_json::Result<String> {
    let canonical = event.hashable_json()?;
    Ok(crate::core::savings_ledger::event::compute_hash(
        event.prev_hash(),
        &canonical,
    ))
}

/// Verifies an ordered in-memory event sequence.
pub fn verify_events(events: &[ExecutionEvent]) -> Result<bool> {
    let mut expected_previous = GENESIS.to_string();
    let mut expected_sequence = 1_u64;
    let mut tasks = HashMap::<String, TaskState>::new();
    let mut receipt_chains = HashMap::<String, (u64, String)>::new();
    let mut canonical_receipt_ids = HashSet::<String>::new();
    let mut admission_ids = HashSet::<String>::new();
    let mut admission_contract_active = false;

    for (index, event) in events.iter().enumerate() {
        if event.sequence_number() != expected_sequence {
            return Ok(false);
        }
        if event.prev_hash() != expected_previous {
            return Ok(false);
        }

        let actual_hash = hash_event(event)?;
        if event
            .entry_hash()
            .is_some_and(|entry_hash| entry_hash != actual_hash)
        {
            return Ok(false);
        }
        if index > 0 && actual_hash.is_empty() {
            return Err(ExecutionLedgerError::InvalidChain(
                "event hash unexpectedly empty".to_owned(),
            ));
        }
        if !validate_lifecycle(
            event,
            &mut tasks,
            &mut receipt_chains,
            &mut canonical_receipt_ids,
            &mut admission_ids,
            &mut admission_contract_active,
        ) {
            return Ok(false);
        }
        expected_previous = actual_hash;
        expected_sequence = expected_sequence.checked_add(1).ok_or_else(|| {
            ExecutionLedgerError::InvalidChain(
                "execution ledger sequence number overflow".to_owned(),
            )
        })?;
    }

    Ok(true)
}

#[derive(Default)]
struct TaskState {
    trace_id: String,
    plans: HashSet<String>,
    invocations: HashSet<String>,
    legacy_receipts: HashSet<String>,
    canonical_receipts: HashSet<String>,
    canonical_outcomes: HashSet<String>,
    outcomes: HashSet<String>,
    decisions: HashSet<String>,
    context_delivered: bool,
    admitted_invocations: HashSet<String>,
    engine_invocations: HashSet<String>,
}

fn validate_lifecycle(
    event: &ExecutionEvent,
    tasks: &mut HashMap<String, TaskState>,
    receipt_chains: &mut HashMap<String, (u64, String)>,
    canonical_receipt_ids: &mut HashSet<String>,
    admission_ids: &mut HashSet<String>,
    admission_contract_active: &mut bool,
) -> bool {
    if event.task_id().is_empty() || event.trace_id().is_empty() || event.timestamp().is_empty() {
        return false;
    }

    if let ExecutionEvent::TaskStarted {
        task_id, trace_id, ..
    } = event
    {
        return tasks
            .insert(
                task_id.clone(),
                TaskState {
                    trace_id: trace_id.clone(),
                    ..TaskState::default()
                },
            )
            .is_none();
    }

    let Some(state) = tasks.get_mut(event.task_id()) else {
        return false;
    };
    if state.trace_id != event.trace_id() {
        return false;
    }

    match event {
        ExecutionEvent::PlanCreated { plan_id, .. } => {
            state.plans.is_empty() && !plan_id.is_empty() && state.plans.insert(plan_id.clone())
        }
        ExecutionEvent::ContextDelivered { .. } => {
            if state.context_delivered || state.plans.is_empty() {
                false
            } else {
                state.context_delivered = true;
                true
            }
        }
        ExecutionEvent::AdmissionConsumed {
            admission_id,
            binding_digest,
            invocation_id,
            ..
        } => {
            if !state.context_delivered
                || admission_id.is_empty()
                || !is_sha256(binding_digest)
                || invocation_id.is_empty()
                || state.engine_invocations.contains(invocation_id)
                || !admission_ids.insert(admission_id.clone())
                || !state.admitted_invocations.insert(invocation_id.clone())
            {
                return false;
            }
            *admission_contract_active = true;
            true
        }
        ExecutionEvent::ModelInvoked {
            plan_id,
            invocation_id,
            invocation_ref,
            provider,
            ..
        } => {
            (state.context_delivered || provider == "savings-ledger")
                && state.plans.contains(plan_id)
                && !invocation_id.is_empty()
                && !invocation_ref.is_empty()
                && state.invocations.insert(invocation_id.clone())
        }
        ExecutionEvent::EngineInvoked {
            plan_id,
            invocation_id,
            invocation_ref,
            capability_id,
            capability_version,
            ..
        } => {
            (!*admission_contract_active || state.admitted_invocations.contains(invocation_id))
                && state.context_delivered
                && state.plans.contains(plan_id)
                && !invocation_id.is_empty()
                && !invocation_ref.is_empty()
                && !capability_id.is_empty()
                && !capability_version.is_empty()
                && state.invocations.insert(invocation_id.clone())
                && state.engine_invocations.insert(invocation_id.clone())
        }
        ExecutionEvent::ReceiptSigned { receipt_id, .. } => {
            !state.invocations.is_empty()
                && !receipt_id.is_empty()
                && state.legacy_receipts.insert(receipt_id.clone())
        }
        ExecutionEvent::CanonicalReceiptRecorded {
            invocation_id,
            receipt_id,
            receipt_ref,
            receipt_digest,
            receipt_chain_id,
            receipt_sequence_number,
            previous_receipt_id,
            previous_signature_digest,
            ..
        } => {
            let predecessor_pair = (
                previous_receipt_id.as_deref(),
                previous_signature_digest.as_deref(),
            );
            let valid_predecessor = match receipt_chains.get(receipt_chain_id) {
                None => *receipt_sequence_number == 1 && predecessor_pair == (None, None),
                Some((sequence, head)) => {
                    sequence.checked_add(1) == Some(*receipt_sequence_number)
                        && previous_receipt_id.as_deref() == Some(head.as_str())
                        && previous_signature_digest.as_deref().is_some_and(is_sha256)
                }
            };
            if !state.context_delivered
                || !state.invocations.contains(invocation_id)
                || !is_sha256(receipt_id)
                || !is_sha256(receipt_digest)
                || receipt_ref != &format!("id:{receipt_digest}")
                || receipt_chain_id.is_empty()
                || !valid_predecessor
                || state.canonical_receipts.contains(receipt_id)
                || canonical_receipt_ids.contains(receipt_id)
            {
                return false;
            }
            state.canonical_receipts.insert(receipt_id.clone());
            canonical_receipt_ids.insert(receipt_id.clone());
            receipt_chains.insert(
                receipt_chain_id.clone(),
                (*receipt_sequence_number, receipt_id.clone()),
            );
            true
        }
        ExecutionEvent::OutcomeRecorded {
            outcome_id,
            receipt_id,
            ..
        } => {
            // ReceiptSigned is legacy compatibility data. Only a canonical
            // receipt may admit one immutable outcome. Exact bundle verification
            // separately validates the linked document and acceptance state.
            state.canonical_receipts.contains(receipt_id)
                && !outcome_id.is_empty()
                && state.canonical_outcomes.insert(receipt_id.clone())
                && state.outcomes.insert(outcome_id.clone())
        }
        ExecutionEvent::DecisionRecorded { decision_id, .. } => {
            !decision_id.is_empty() && state.decisions.insert(decision_id.clone())
        }
        ExecutionEvent::TaskStarted { .. } => false,
    }
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::execution_ledger::event::ContextBalanceV1;
    use crate::core::execution_ledger::{ExecutionLedgerError, ExecutionLedgerStore};
    use tempfile::tempdir;

    fn chain(mut events: Vec<ExecutionEvent>) -> Vec<ExecutionEvent> {
        let mut previous = GENESIS.to_owned();
        for (index, event) in events.iter_mut().enumerate() {
            event.set_chain_fields(index as u64 + 1, previous);
            previous = hash_event(event).expect("fixture event hash");
        }
        events
    }

    fn started(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::TaskStarted {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            envelope_ref: "envelope:test".to_owned(),
            timestamp: "2026-08-24T12:00:00Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn plan(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::PlanCreated {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            plan_id: "plan:test".to_owned(),
            plan_ref: "sha256:plan".to_owned(),
            timestamp: "2026-08-24T12:00:01Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn context(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::ContextDelivered {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            context_balance: ContextBalanceV1 {
                original_tokens: 3,
                materialized_tokens: 2,
                delivered_tokens: 2,
                provider_billed_tokens: 2,
            },
            timestamp: "2026-08-24T12:00:02Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn admission(task_id: &str, trace_id: &str, admission_id: &str) -> ExecutionEvent {
        ExecutionEvent::AdmissionConsumed {
            admission_id: admission_id.to_owned(),
            binding_digest: format!("sha256:{}", "a".repeat(64)),
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            invocation_id: "invocation:test".to_owned(),
            timestamp: "2026-08-24T12:00:03Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn engine(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::EngineInvoked {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            plan_id: "plan:test".to_owned(),
            invocation_id: "invocation:test".to_owned(),
            invocation_ref: "sha256:invocation".to_owned(),
            capability_id: "capability:test".to_owned(),
            capability_version: "1.0.0".to_owned(),
            timestamp: "2026-08-24T12:00:04Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn prefix(task_id: &str, trace_id: &str) -> Vec<ExecutionEvent> {
        vec![
            started(task_id, trace_id),
            plan(task_id, trace_id),
            context(task_id, trace_id),
        ]
    }

    fn append_prefix(store: &ExecutionLedgerStore, task_id: &str, trace_id: &str) {
        for event in prefix(task_id, trace_id) {
            store.append(event).expect("append lifecycle prefix");
        }
    }

    #[test]
    fn admission_must_precede_engine_after_contract_activation() {
        let mut events = prefix("task:test", "trace:test");
        events.push(admission("task:test", "trace:test", "admission:test"));
        events.push(engine("task:test", "trace:test"));
        assert!(verify_events(&chain(events)).unwrap());

        let mut missing = prefix("task:one", "trace:one");
        missing.push(admission("task:one", "trace:one", "admission:one"));
        missing.extend(prefix("task:two", "trace:two"));
        missing.push(engine("task:two", "trace:two"));
        assert!(!verify_events(&chain(missing)).unwrap());
    }

    #[test]
    fn admission_rejects_pre_context_post_engine_and_malformed_digest() {
        let mut pre_context = vec![
            started("task:test", "trace:test"),
            plan("task:test", "trace:test"),
        ];
        pre_context.push(admission("task:test", "trace:test", "admission:test"));
        assert!(!verify_events(&chain(pre_context)).unwrap());

        let mut post_engine = prefix("task:test", "trace:test");
        post_engine.push(engine("task:test", "trace:test"));
        post_engine.push(admission("task:test", "trace:test", "admission:test"));
        assert!(!verify_events(&chain(post_engine)).unwrap());

        let mut malformed = prefix("task:test", "trace:test");
        let mut event = admission("task:test", "trace:test", "admission:test");
        if let ExecutionEvent::AdmissionConsumed { binding_digest, .. } = &mut event {
            *binding_digest = "sha256:not-a-digest".to_owned();
        }
        malformed.push(event);
        assert!(!verify_events(&chain(malformed)).unwrap());
    }

    #[test]
    fn admission_ids_are_global_and_historical_engines_remain_readable() {
        let mut historical = prefix("task:old", "trace:old");
        historical.push(engine("task:old", "trace:old"));
        assert!(verify_events(&chain(historical)).unwrap());

        let mut sequential = prefix("task:seq", "trace:seq");
        sequential.push(admission("task:seq", "trace:seq", "admission:first"));
        sequential.push(engine("task:seq", "trace:seq"));
        let mut second_admission = admission("task:seq", "trace:seq", "admission:second");
        let mut second_engine = engine("task:seq", "trace:seq");
        if let ExecutionEvent::AdmissionConsumed { invocation_id, .. } = &mut second_admission {
            *invocation_id = "invocation:second".to_owned();
        }
        if let ExecutionEvent::EngineInvoked { invocation_id, .. } = &mut second_engine {
            *invocation_id = "invocation:second".to_owned();
        }
        sequential.push(second_admission);
        sequential.push(second_engine);
        assert!(verify_events(&chain(sequential)).unwrap());

        let mut duplicate = prefix("task:one", "trace:one");
        duplicate.push(admission("task:one", "trace:one", "admission:global"));
        duplicate.extend(prefix("task:two", "trace:two"));
        duplicate.push(admission("task:two", "trace:two", "admission:global"));
        assert!(!verify_events(&chain(duplicate)).unwrap());
    }

    #[test]
    fn admission_hash_and_canonical_json_are_deterministic() {
        let event = admission("task:test", "trace:test", "admission:test");
        assert_eq!(
            event.canonical_json().unwrap(),
            "{\"AdmissionConsumed\":{\"admission_id\":\"admission:test\",\"binding_digest\":\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"task_id\":\"task:test\",\"trace_id\":\"trace:test\",\"invocation_id\":\"invocation:test\",\"timestamp\":\"2026-08-24T12:00:03Z\",\"sequence_number\":0,\"prev_hash\":\"\"}}"
        );
        assert_eq!(
            event.event_hash().unwrap(),
            "647e5f54641fd974cd8741c13aeda68bfe2501319bc7981d62dbddb127585edb"
        );
        let mut changed = event.clone();
        if let ExecutionEvent::AdmissionConsumed { binding_digest, .. } = &mut changed {
            *binding_digest = format!("sha256:{}", "b".repeat(64));
        }
        assert_ne!(event.event_hash().unwrap(), changed.event_hash().unwrap());
        assert_eq!(
            event.idempotency_key(),
            Some(("admission", "admission:test", ""))
        );
    }

    #[test]
    fn store_retry_is_false_and_conflicting_payload_does_not_mutate_bytes() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        append_prefix(&store, "task:store", "trace:store");
        let event = admission("task:store", "trace:store", "admission:store");

        assert!(store.append_if_new(event.clone()).unwrap());
        assert!(!store.append_if_new(event).unwrap());
        let before_conflict = std::fs::read(&path).expect("ledger bytes");

        let mut changed = admission("task:store", "trace:store", "admission:store");
        if let ExecutionEvent::AdmissionConsumed { binding_digest, .. } = &mut changed {
            *binding_digest = format!("sha256:{}", "b".repeat(64));
        }
        assert!(matches!(
            store.append_if_new(changed),
            Err(ExecutionLedgerError::InvalidRecord(_))
        ));
        assert_eq!(std::fs::read(&path).expect("ledger bytes"), before_conflict);
    }

    #[test]
    fn store_admission_identity_is_global_across_tasks() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        append_prefix(&store, "task:one", "trace:one");
        assert!(
            store
                .append_if_new(admission("task:one", "trace:one", "admission:global"))
                .unwrap()
        );
        append_prefix(&store, "task:two", "trace:two");

        assert!(matches!(
            store.append_if_new(admission("task:two", "trace:two", "admission:global")),
            Err(ExecutionLedgerError::InvalidRecord(_))
        ));
        assert_eq!(store.load().unwrap().len(), 7);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_admission_append_has_exactly_one_winner() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        append_prefix(&store, "task:concurrent", "trace:concurrent");
        let event = admission(
            "task:concurrent",
            "trace:concurrent",
            "admission:concurrent",
        );
        let workers = 8;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));
        let handles = (0..workers)
            .map(|_| {
                let store = store.clone();
                let barrier = barrier.clone();
                let event = event.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_if_new(event)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("append worker"))
            .collect::<Result<Vec<_>>>();
        let results = results.expect("concurrent append result");
        assert_eq!(results.iter().filter(|result| **result).count(), 1);
        assert_eq!(
            results.iter().filter(|result| !**result).count(),
            workers - 1
        );
        assert_eq!(store.load().unwrap().len(), 4);
    }
}
