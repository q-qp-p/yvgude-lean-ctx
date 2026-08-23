//! Append-only, task-aware execution ledger.
//!
//! The execution ledger is an additive projection beside the Savings Ledger.  It
//! shares the Savings Ledger's SHA-256 chain primitive, but stores execution
//! identity and lifecycle observations keyed by task and trace IDs.

pub mod canonical_receipt;
pub mod event;
pub mod migration;
pub mod producer;
pub mod projection;
pub mod store;
pub mod verify;

pub use canonical_receipt::{PublishedCanonicalReceipt, publish_canonical_receipt};
pub use event::{ContextBalance, ContextBalanceV1, ExecutionEvent, TriState};
pub use migration::{migrate_from_savings_ledger, migrate_from_savings_ledger_at};
pub use producer::{
    CanonicalReceiptRecordV1, ReceiptSignerAdmissionV1, record_canonical_engine_receipt,
};
pub use projection::{
    CanonicalReceiptProjectionV1, ExecutionProjection, TaskCostSummary,
    canonical_receipt_for_task_from_store, canonical_receipt_for_task_verified_from_store,
    receipt_for_task, receipt_for_task_from_store, task_cost_summary, task_cost_summary_from_store,
};
pub use store::{ExecutionLedgerStore, default_path};
pub use verify::{GENESIS, hash_event, verify_events};

use std::io;

use lean_ctx_protocol::ExecutionReceiptV1;
use serde::{Deserialize, Serialize};

/// Errors returned while reading, serializing, or validating the execution ledger.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionLedgerError {
    #[error("execution ledger I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("execution ledger serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("execution ledger chain is invalid: {0}")]
    InvalidChain(String),
    #[error("execution ledger record is invalid: {0}")]
    InvalidRecord(String),
}

/// Result type shared by execution-ledger operations.
pub type Result<T> = std::result::Result<T, ExecutionLedgerError>;

/// Compatibility view consumed by the existing `ledger execution` CLI.
///
/// The event store remains the source of truth; this view groups the current
/// task projection without introducing a second writable ledger format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLedgerEntryV1 {
    pub task_id: String,
    pub receipt: Option<ExecutionReceiptV1>,
    pub actual_cost_micros: Option<u64>,
    pub baseline_cost_micros: Option<u64>,
    pub predicted_cost_micros: Option<u64>,
    pub actual_etpao: Option<crate::core::etpao::EtpaoResult>,
    pub baseline_etpao: Option<crate::core::etpao::EtpaoResult>,
    pub predicted_etpao: Option<crate::core::etpao::EtpaoResult>,
}

/// Compatibility aggregate for the existing execution-ledger CLI commands.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionLedger {
    pub entries: Vec<ExecutionLedgerEntryV1>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionLedgerVerifyResult {
    pub valid: bool,
    pub total_entries: usize,
    pub first_invalid_at: Option<usize>,
    pub error: Option<String>,
}

impl ExecutionLedger {
    /// Loads the compatibility projection from the default event store.
    pub fn load() -> Result<Self> {
        let store = ExecutionLedgerStore::from_default()?;
        let events = store.load_verified()?;
        let mut task_ids = Vec::new();
        for event in events {
            if !task_ids.iter().any(|known| known == event.task_id()) {
                task_ids.push(event.task_id().to_owned());
            }
        }

        let entries = task_ids
            .into_iter()
            .map(|task_id| {
                let receipt = receipt_for_task_from_store(&store, &task_id);
                ExecutionLedgerEntryV1 {
                    task_id,
                    actual_cost_micros: receipt.as_ref().map(|value| value.actual_cost_micros),
                    baseline_cost_micros: receipt.as_ref().map(|value| value.baseline_cost_micros),
                    predicted_cost_micros: None,
                    actual_etpao: None,
                    baseline_etpao: None,
                    predicted_etpao: None,
                    receipt,
                }
            })
            .collect();
        Ok(Self { entries })
    }

    /// Finds the first projection for a task.
    #[must_use]
    pub fn find_task(&self, task_id: &str) -> Option<&ExecutionLedgerEntryV1> {
        self.entries.iter().find(|entry| entry.task_id == task_id)
    }

    /// Verifies the source event chain and reports CLI-friendly details.
    #[must_use]
    pub fn verify(&self) -> ExecutionLedgerVerifyResult {
        match ExecutionLedgerStore::from_default().and_then(|store| store.verify_chain()) {
            Ok(valid) => ExecutionLedgerVerifyResult {
                valid,
                total_entries: self.entries.len(),
                first_invalid_at: (!valid).then_some(0),
                error: (!valid).then(|| "hash or sequence link mismatch".to_owned()),
            },
            Err(error) => ExecutionLedgerVerifyResult {
                valid: false,
                total_entries: self.entries.len(),
                first_invalid_at: Some(0),
                error: Some(error.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lean_ctx_protocol::AcceptanceState;
    use tempfile::tempdir;

    fn task_started(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::TaskStarted {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            envelope_ref: "envelope:task".to_owned(),
            timestamp: "2026-08-09T12:00:00Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn model_invoked(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::ModelInvoked {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            plan_id: "plan-1".to_owned(),
            invocation_id: "invocation-1".to_owned(),
            invocation_ref: "sha256:invocation-1".to_owned(),
            model: "model-1".to_owned(),
            provider: "provider-1".to_owned(),
            tokens_in: 20,
            tokens_out: 10,
            latency_ms: 25,
            timestamp: "2026-08-09T12:00:01Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn context_delivered(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::ContextDelivered {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            context_balance: ContextBalanceV1 {
                original_tokens: 100,
                materialized_tokens: 80,
                delivered_tokens: 60,
                provider_billed_tokens: 60,
            },
            timestamp: "2026-08-09T12:00:00Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn engine_invoked(task_id: &str, trace_id: &str) -> ExecutionEvent {
        ExecutionEvent::EngineInvoked {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            plan_id: "plan-1".to_owned(),
            invocation_id: "invocation-engine-1".to_owned(),
            invocation_ref: "sha256:engine-invocation-1".to_owned(),
            capability_id: "capability://leanctx/context".to_owned(),
            capability_version: "1.0.0".to_owned(),
            timestamp: "2026-08-09T12:00:01Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn canonical_record(
        task_id: &str,
        trace_id: &str,
        invocation_id: &str,
        digit: char,
    ) -> ExecutionEvent {
        ExecutionEvent::CanonicalReceiptRecorded {
            task_id: task_id.to_owned(),
            trace_id: trace_id.to_owned(),
            invocation_id: invocation_id.to_owned(),
            receipt_id: format!("sha256:{}", digit.to_string().repeat(64)),
            receipt_ref: format!("id:sha256:{}", digit.to_string().repeat(64)),
            receipt_digest: format!("sha256:{}", digit.to_string().repeat(64)),
            receipt_chain_id: "shared-chain".to_owned(),
            receipt_sequence_number: 1,
            previous_receipt_id: None,
            previous_signature_digest: None,
            timestamp: "2026-08-23T12:00:00Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
            entry_hash: String::new(),
        }
    }

    #[test]
    fn append_assigns_sequence_and_verifies_chain() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        store.append(task_started("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "plan:1".to_owned(),
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(context_delivered("task-1", "trace-1"))
            .unwrap();
        store.append(model_invoked("task-1", "trace-1")).unwrap();

        assert_eq!(store.last_sequence(), 4);
        assert!(store.verify_chain().unwrap());
    }

    #[test]
    fn engine_invocation_is_auditable_without_fabricated_model_usage() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        store.append(task_started("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "sha256:plan-1".to_owned(),
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(context_delivered("task-1", "trace-1"))
            .unwrap();
        store.append(engine_invoked("task-1", "trace-1")).unwrap();

        assert!(store.verify_chain().unwrap());
        assert_eq!(store.task_cost_summary("task-1").model_calls, 0);
        assert!(store.receipt_for_task("task-1").is_none());
    }

    #[test]
    fn canonical_receipt_self_hash_detects_last_event_tampering() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "plan:1".to_owned(),
                timestamp: "2026-08-23T11:59:58Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(ExecutionEvent::ContextDelivered {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                context_balance: ContextBalanceV1 {
                    original_tokens: 1,
                    materialized_tokens: 1,
                    delivered_tokens: 1,
                    provider_billed_tokens: 1,
                },
                timestamp: "2026-08-23T11:59:58Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store.append(model_invoked("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::CanonicalReceiptRecorded {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                invocation_id: "invocation-1".to_owned(),
                receipt_id: format!("sha256:{}", "1".repeat(64)),
                receipt_ref: format!("id:sha256:{}", "2".repeat(64)),
                receipt_digest: format!("sha256:{}", "2".repeat(64)),
                receipt_chain_id: "chain-1".to_owned(),
                receipt_sequence_number: 1,
                previous_receipt_id: None,
                previous_signature_digest: None,
                timestamp: "2026-08-23T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
                entry_hash: String::new(),
            })
            .unwrap();
        assert!(store.verify_chain().unwrap());
        let projected = store.canonical_receipt_for_task("task-1").unwrap();
        assert_eq!(projected.invocation_id, "invocation-1");
        assert_eq!(projected.chain_id, "chain-1");
        assert_eq!(projected.sequence_number, 1);

        let content = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, content.replace(&"1".repeat(64), &"3".repeat(64))).unwrap();

        assert!(!store.verify_chain().unwrap());
    }

    #[test]
    fn verify_chain_rejects_modified_event() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "plan:1".to_owned(),
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(context_delivered("task-1", "trace-1"))
            .unwrap();
        store.append(model_invoked("task-1", "trace-1")).unwrap();
        let mut line = std::fs::read_to_string(&path).unwrap();
        line = line.replace("envelope:task", "envelope:changed");
        std::fs::write(&path, line).unwrap();

        assert!(!store.verify_chain().unwrap());
    }

    #[test]
    fn record_digest_detects_single_tail_event_tampering() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started("task-1", "trace-1")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, content.replace("trace-1", "trace-x")).unwrap();

        assert!(!store.verify_chain().unwrap());
    }

    #[test]
    fn append_rejects_cross_trace_and_out_of_order_events() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        store.append(task_started("task-1", "trace-1")).unwrap();

        assert!(matches!(
            store.append(model_invoked("task-1", "trace-1")),
            Err(ExecutionLedgerError::InvalidChain(_))
        ));
        assert!(matches!(
            store.append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-x".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "plan:1".to_owned(),
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            }),
            Err(ExecutionLedgerError::InvalidChain(_))
        ));
        assert_eq!(store.last_sequence(), 1);
    }

    #[test]
    fn canonical_receipt_chain_rejects_cross_task_forks_and_unlinked_outcomes() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        for (task_id, trace_id) in [("task-1", "trace-1"), ("task-2", "trace-2")] {
            store.append(task_started(task_id, trace_id)).unwrap();
            store
                .append(ExecutionEvent::PlanCreated {
                    task_id: task_id.to_owned(),
                    trace_id: trace_id.to_owned(),
                    plan_id: "plan-1".to_owned(),
                    plan_ref: "plan:1".to_owned(),
                    timestamp: "2026-08-23T11:59:00Z".to_owned(),
                    sequence_number: 0,
                    prev_hash: String::new(),
                })
                .unwrap();
            store.append(context_delivered(task_id, trace_id)).unwrap();
            store.append(engine_invoked(task_id, trace_id)).unwrap();
        }
        assert!(matches!(
            store.append(ExecutionEvent::OutcomeRecorded {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                outcome_id: "outcome-1".to_owned(),
                receipt_id: format!("sha256:{}", "1".repeat(64)),
                accepted: AcceptanceState::Accepted,
                timestamp: "2026-08-23T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            }),
            Err(ExecutionLedgerError::InvalidChain(_))
        ));
        store
            .append(canonical_record(
                "task-1",
                "trace-1",
                "invocation-engine-1",
                '1',
            ))
            .unwrap();
        assert!(matches!(
            store.append(canonical_record(
                "task-2",
                "trace-2",
                "invocation-engine-1",
                '2',
            )),
            Err(ExecutionLedgerError::InvalidChain(_))
        ));
    }

    #[test]
    fn load_rejects_duplicate_json_keys() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started("task-1", "trace-1")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let duplicated = content.replacen(
            "\"task_id\":\"task-1\",",
            "\"task_id\":\"task-1\",\"task_id\":\"task-1\",",
            1,
        );
        std::fs::write(&path, duplicated).unwrap();

        assert!(matches!(
            store.load(),
            Err(ExecutionLedgerError::Serialization(_))
        ));
    }

    #[test]
    fn load_rejects_noncanonical_json_whitespace() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started("task-1", "trace-1")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!(" {content}")).unwrap();

        assert!(matches!(
            store.load(),
            Err(ExecutionLedgerError::InvalidRecord(message))
                if message == "execution ledger record is not canonical JSON"
        ));
    }

    #[test]
    fn by_task_filters_events_without_losing_order() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        store.append(task_started("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "plan:1".to_owned(),
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store.append(task_started("task-2", "trace-2")).unwrap();
        store
            .append(context_delivered("task-1", "trace-1"))
            .unwrap();
        store.append(model_invoked("task-1", "trace-1")).unwrap();

        let events = store.by_task("task-1");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].sequence_number(), 1);
        assert_eq!(events[1].sequence_number(), 2);
        assert_eq!(events[2].sequence_number(), 4);
        assert_eq!(events[3].sequence_number(), 5);
    }

    #[test]
    fn migration_reads_source_without_mutating_it() {
        let directory = tempdir().expect("temporary directory");
        let savings_path = directory.path().join("savings.jsonl");
        let execution_path = directory.path().join("execution.jsonl");
        let savings_event = crate::core::savings_ledger::event::SavingsEvent {
            ts: "2026-08-09T12:00:00Z".to_owned(),
            tool: "ctx_read".to_owned(),
            mechanism: "compression".to_owned(),
            model_id: "model-1".to_owned(),
            tokenizer: "o200k_base".to_owned(),
            baseline_tokens: 100,
            actual_tokens: 60,
            saved_tokens: 40,
            bounce_adjustment: 0,
            unit_price_per_m_usd: 1.0,
            saved_usd: 0.00004,
            repo_hash: "repo".to_owned(),
            agent_id: "agent".to_owned(),
            prev_hash: String::new(),
            entry_hash: String::new(),
            version: "test".to_owned(),
            intent_tag: None,
            outcome: None,
            model_original: None,
            model_routed: None,
            routing_savings: None,
            response_original_tokens: None,
            response_delivered_tokens: None,
            agent_chain_id: None,
            chain_depth: None,
            measurement_method: None,
            evidence_class: None,
            confidence: None,
            request_id: None,
            session_id: None,
            trace_id: None,
            solution_decision: None,
            loc_added: None,
            loc_removed: None,
            path: None,
            lines_added: None,
            lines_removed: None,
            net: None,
            quality_signal: None,
            attribution_group: None,
            attribution_id: None,
            baseline_ref: None,
            price_version: None,
            customer_approval: None,
            settlement_status: None,
            is_first_inject: None,
            cache_read_per_m_usd: None,
            cache_write_per_m_usd: None,
        };
        let mut second_savings_event = savings_event.clone();
        second_savings_event.ts = "2026-08-09T12:00:01Z".to_owned();
        second_savings_event.model_id = "model-2".to_owned();
        crate::core::savings_ledger::store::append(&savings_path, savings_event).unwrap();
        crate::core::savings_ledger::store::append(&savings_path, second_savings_event).unwrap();
        let before = std::fs::read(&savings_path).unwrap();
        let source_hashes: Vec<String> = crate::core::savings_ledger::store::load(&savings_path)
            .into_iter()
            .map(|event| event.entry_hash)
            .collect();

        let destination = ExecutionLedgerStore::new(execution_path);
        assert_eq!(
            migrate_from_savings_ledger_at(
                &savings_path,
                &destination,
                "task-legacy",
                "trace-legacy"
            )
            .unwrap(),
            2
        );
        assert_eq!(before, std::fs::read(&savings_path).unwrap());
        assert_eq!(
            migrate_from_savings_ledger_at(
                &savings_path,
                &destination,
                "task-legacy",
                "trace-legacy"
            )
            .unwrap(),
            0
        );
        let migrated_events = destination.by_task("task-legacy");
        assert_eq!(migrated_events.len(), 4);
        assert_eq!(
            migrated_events
                .iter()
                .filter(|event| matches!(event, ExecutionEvent::PlanCreated { .. }))
                .count(),
            1
        );
        for source_hash in source_hashes {
            assert!(migrated_events.iter().any(|event| {
                matches!(event, ExecutionEvent::ModelInvoked { invocation_ref, .. } if invocation_ref.contains(&source_hash))
            }));
        }
    }

    #[test]
    fn projections_summarize_cost_and_build_receipt() {
        let directory = tempdir().expect("temporary directory");
        let store = ExecutionLedgerStore::new(directory.path().join("ledger.jsonl"));
        store.append(task_started("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::PlanCreated {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                plan_id: "plan-1".to_owned(),
                plan_ref: "plan:1".to_owned(),
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(ExecutionEvent::ContextDelivered {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                context_balance: ContextBalanceV1 {
                    original_tokens: 100,
                    materialized_tokens: 80,
                    delivered_tokens: 60,
                    provider_billed_tokens: 60,
                },
                timestamp: "2026-08-09T12:00:00Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(context_delivered("task-1", "trace-1"))
            .unwrap();
        store.append(model_invoked("task-1", "trace-1")).unwrap();
        store
            .append(ExecutionEvent::ReceiptSigned {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                receipt_id: "receipt-1".to_owned(),
                receipt_hash: "sha256:receipt".to_owned(),
                signature: "signature".to_owned(),
                timestamp: "2026-08-09T12:00:02Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();
        store
            .append(ExecutionEvent::OutcomeRecorded {
                task_id: "task-1".to_owned(),
                trace_id: "trace-1".to_owned(),
                outcome_id: "outcome-1".to_owned(),
                receipt_id: "receipt-1".to_owned(),
                accepted: AcceptanceState::Accepted,
                timestamp: "2026-08-09T12:00:03Z".to_owned(),
                sequence_number: 0,
                prev_hash: String::new(),
            })
            .unwrap();

        let summary = task_cost_summary_from_store(&store, "task-1");
        assert_eq!(summary.total_tokens, 30);
        assert_eq!(summary.model_calls, 1);
        let receipt = receipt_for_task_from_store(&store, "task-1").unwrap();
        assert_eq!(receipt.model_calls, 1);
        assert_eq!(receipt.output_tokens, 10);
        assert_eq!(receipt.outcome_ref.as_deref(), Some("outcome-1"));
    }
}
