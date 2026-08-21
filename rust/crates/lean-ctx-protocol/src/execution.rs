//! Planning and execution receipt contracts.

use crate::common::{
    CapabilityId, PlanId, ReceiptId, TaskId, ValidationError, deserialize_milliunit,
    deserialize_schema_version, validate_milliunit, validate_schema_version,
};
use crate::evidence::EvidenceRefV1;
use serde::{Deserialize, Serialize};

/// Strategy used to assemble context for a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    Minimal,
    Balanced,
    Comprehensive,
    CachedFirst,
}

/// Terminal condition selected by an execution plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopCondition {
    OnCompletion,
    OnAcceptance,
    OnBudgetExhaustion,
    OnError,
    Manual,
}

/// V1 execution plan produced from a task envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlanV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub plan_id: PlanId,
    pub task_id: TaskId,
    pub context_budget_tokens: u64,
    pub context_strategy: ContextStrategy,
    pub knowledge_refs: Vec<String>,
    pub capability_ids: Vec<CapabilityId>,
    pub model: String,
    pub provider: String,
    #[serde(deserialize_with = "deserialize_milliunit")]
    pub reasoning_allocation_milli: u16,
    pub max_retries: u32,
    pub fallback_refs: Vec<String>,
    pub stop_condition: StopCondition,
    pub expected_cost_micros: u64,
    #[serde(deserialize_with = "deserialize_milliunit")]
    pub expected_quality_milli: u16,
    pub expected_latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_decision_ref: Option<String>,
}

impl ExecutionPlanV1 {
    /// Validate invariants that also apply to values constructed in Rust.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        validate_milliunit(
            self.reasoning_allocation_milli,
            "reasoning_allocation_milli",
        )?;
        validate_milliunit(self.expected_quality_milli, "expected_quality_milli")?;
        Ok(())
    }
}

/// Four-stage token balance carried by an execution receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBalanceV1 {
    pub original_tokens: u64,
    pub materialized_tokens: u64,
    pub delivered_tokens: u64,
    pub provider_billed_tokens: u64,
}

impl ContextBalanceV1 {
    /// Validate monotonic context accounting across the four stages.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.materialized_tokens > self.original_tokens {
            return Err(ValidationError::new(
                "materialized_tokens exceeds original_tokens",
            ));
        }
        if self.delivered_tokens > self.materialized_tokens {
            return Err(ValidationError::new(
                "delivered_tokens exceeds materialized_tokens",
            ));
        }
        Ok(())
    }
}

/// Auditable result of executing an execution plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceiptV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub receipt_id: ReceiptId,
    pub task_id: TaskId,
    pub plan_id: PlanId,
    pub context_balance: ContextBalanceV1,
    pub fresh_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub requested_model: String,
    pub selected_model: String,
    pub provider: String,
    /// Capability that produced this receipt, when the producer is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    /// Version of the capability that produced this receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_version: Option<String>,
    pub model_calls: u32,
    pub retries: u32,
    pub latency_ms: u64,
    pub actual_cost_micros: u64,
    pub baseline_cost_micros: u64,
    pub avoided_cost_micros: u64,
    pub etpao_milli: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_refs: Vec<String>,
    pub decision_refs: Vec<String>,
    pub evidence_refs: Vec<EvidenceRefV1>,
    pub signature: String,
}

impl ExecutionReceiptV1 {
    /// Validate receipt accounting and schema invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        self.context_balance.validate()?;
        if self.avoided_cost_micros > self.baseline_cost_micros {
            return Err(ValidationError::new(
                "avoided_cost_micros exceeds baseline_cost_micros",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T>(value: &str) -> T
    where
        T: TryFrom<String>,
        <T as TryFrom<String>>::Error: std::fmt::Debug,
    {
        T::try_from(value.to_owned()).expect("identifier should be valid")
    }

    fn balance() -> ContextBalanceV1 {
        ContextBalanceV1 {
            original_tokens: 1_000,
            materialized_tokens: 800,
            delivered_tokens: 700,
            provider_billed_tokens: 650,
        }
    }

    #[test]
    fn plan_serialization_round_trip() {
        let plan = ExecutionPlanV1 {
            schema_version: 1,
            plan_id: id("plan-1"),
            task_id: id("task-1"),
            context_budget_tokens: 10_000,
            context_strategy: ContextStrategy::Balanced,
            knowledge_refs: vec!["knowledge:1".to_owned()],
            capability_ids: vec![id("capability:search")],
            model: "model-1".to_owned(),
            provider: "provider-1".to_owned(),
            reasoning_allocation_milli: 500,
            max_retries: 2,
            fallback_refs: vec!["model:fallback".to_owned()],
            stop_condition: StopCondition::OnAcceptance,
            expected_cost_micros: 5_000,
            expected_quality_milli: 850,
            expected_latency_ms: 1_500,
            policy_decision_ref: Some("decision:policy".to_owned()),
            scheduler_decision_ref: Some("decision:scheduler".to_owned()),
        };
        let json = serde_json::to_string(&plan).expect("plan should serialize");
        let decoded: ExecutionPlanV1 =
            serde_json::from_str(&json).expect("plan should deserialize");
        assert_eq!(plan, decoded);
        plan.validate().expect("plan should satisfy invariants");
    }

    #[test]
    fn receipt_serialization_round_trip() {
        let receipt = ExecutionReceiptV1 {
            schema_version: 1,
            receipt_id: id("receipt-1"),
            task_id: id("task-1"),
            plan_id: id("plan-1"),
            context_balance: balance(),
            fresh_input_tokens: 600,
            cached_input_tokens: 100,
            output_tokens: 200,
            reasoning_tokens: 50,
            requested_model: "model-requested".to_owned(),
            selected_model: "model-selected".to_owned(),
            provider: "provider-1".to_owned(),
            capability_id: Some("capability://leanctx/context".to_owned()),
            capability_version: Some("1.0.0".to_owned()),
            model_calls: 2,
            retries: 1,
            latency_ms: 900,
            actual_cost_micros: 2_000,
            baseline_cost_micros: 3_000,
            avoided_cost_micros: 1_000,
            etpao_milli: 1_250,
            outcome_ref: Some("outcome:1".to_owned()),
            knowledge_refs: vec!["knowledge:1".to_owned()],
            decision_refs: vec!["decision:1".to_owned()],
            evidence_refs: vec![],
            signature: "signature".to_owned(),
        };
        let json = serde_json::to_string(&receipt).expect("receipt should serialize");
        let decoded: ExecutionReceiptV1 =
            serde_json::from_str(&json).expect("receipt should deserialize");
        assert_eq!(receipt, decoded);
        receipt
            .validate()
            .expect("receipt should satisfy invariants");
    }

    #[test]
    fn receipt_capability_metadata_is_optional_and_backward_compatible() {
        let receipt = ExecutionReceiptV1 {
            schema_version: 1,
            receipt_id: id("receipt-1"),
            task_id: id("task-1"),
            plan_id: id("plan-1"),
            context_balance: balance(),
            fresh_input_tokens: 600,
            cached_input_tokens: 100,
            output_tokens: 200,
            reasoning_tokens: 50,
            requested_model: "model-requested".to_owned(),
            selected_model: "model-selected".to_owned(),
            provider: "provider-1".to_owned(),
            capability_id: Some("capability://leanctx/context".to_owned()),
            capability_version: Some("1.0.0".to_owned()),
            model_calls: 2,
            retries: 1,
            latency_ms: 900,
            actual_cost_micros: 2_000,
            baseline_cost_micros: 3_000,
            avoided_cost_micros: 1_000,
            etpao_milli: 1_250,
            outcome_ref: Some("outcome:1".to_owned()),
            knowledge_refs: vec!["knowledge:1".to_owned()],
            decision_refs: vec!["decision:1".to_owned()],
            evidence_refs: vec![],
            signature: "signature".to_owned(),
        };
        let json = serde_json::to_value(&receipt).expect("receipt should serialize");
        assert_eq!(json["capability_id"], "capability://leanctx/context");
        assert_eq!(
            serde_json::from_value::<ExecutionReceiptV1>(json.clone())
                .expect("receipt with capability metadata should deserialize"),
            receipt
        );

        let mut legacy = json;
        let object = legacy
            .as_object_mut()
            .expect("serialized receipt should be an object");
        object.remove("capability_id");
        object.remove("capability_version");

        let decoded: ExecutionReceiptV1 =
            serde_json::from_value(legacy).expect("legacy receipt should deserialize");
        assert_eq!(decoded.capability_id, None);
        assert_eq!(decoded.capability_version, None);

        let without_capability = serde_json::to_value(decoded)
            .expect("receipt without capability metadata should serialize");
        assert!(without_capability.get("capability_id").is_none());
        assert!(without_capability.get("capability_version").is_none());
    }
}
