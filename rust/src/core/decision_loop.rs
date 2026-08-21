//! End-to-end wiring from task triage through execution-value assessment.

use lean_ctx_protocol::{RiskClass, TaskComplexity, TaskEnvelopeV1, TaskProfileV1, TaskScope};

use crate::core::{
    task_spine::TaskSpine,
    triage::{
        TaskAnalysisInput, TriageEngine,
        profile::{TaskProfileLocal, TaskScopeLocal},
    },
    value_gate::{self, ExecutionCost, OutcomeSignal, TaskOutcome, ValueAssessment, ValueGate},
};

#[derive(Debug, Clone)]
/// Orchestrates triage, spine, and value gate for a complete task evaluation.
pub struct DecisionLoop {
    triage: TriageEngine,
    value_gate: ValueGate,
}

#[derive(Debug, Clone)]
/// Holds the task state and value assessment from a decision-loop execution.
pub struct DecisionResult {
    pub task_id: String,
    /// The ingress envelope enriched by triage; its task id is the lineage root.
    pub envelope: TaskEnvelopeV1,
    pub profile: TaskProfileLocal,
    pub envelope_created: bool,
    pub cost: Option<ExecutionCost>,
    pub outcome: Option<TaskOutcome>,
    pub assessment: Option<ValueAssessment>,
}

impl Default for DecisionLoop {
    fn default() -> Self {
        Self::new(TriageEngine::default(), ValueGate)
    }
}

impl DecisionLoop {
    pub fn new(triage: TriageEngine, value_gate: ValueGate) -> Self {
        Self { triage, value_gate }
    }

    pub fn execute_task(&self, query: &str, session_id: &str, agent_id: &str) -> DecisionResult {
        let profile = self
            .triage
            .analyze(&TaskAnalysisInput {
                query: query.to_owned(),
                ..Default::default()
            })
            .map(|hypothesis| hypothesis.profile)
            .unwrap_or_default();
        let mut envelope = TaskSpine::create_envelope(query, session_id, agent_id);
        TaskSpine::enrich_from_triage(&mut envelope, &protocol_profile(&profile));

        DecisionResult {
            task_id: envelope.task_id.as_str().to_owned(),
            envelope,
            profile,
            envelope_created: true,
            cost: None,
            outcome: None,
            assessment: None,
        }
    }

    pub fn complete_task(
        &self,
        result: &mut DecisionResult,
        cost: ExecutionCost,
        signals: Vec<OutcomeSignal>,
    ) {
        let outcome = TaskOutcome {
            task_id: result.task_id.clone(),
            completed: true,
            signals,
        };
        let ValueGate = self.value_gate;
        let assessment = ValueGate::evaluate_task(&result.task_id, &cost, &outcome);
        result.cost = Some(cost);
        result.outcome = Some(outcome);
        result.assessment = Some(assessment);
    }

    pub fn aggregate_cpao(results: &[DecisionResult]) -> Option<u64> {
        let (costs, accepted): (Vec<_>, Vec<_>) = results
            .iter()
            .filter_map(|result| {
                Some((
                    result.cost.as_ref()?.estimated_cost_micros,
                    result.assessment.as_ref()?.outcome_accepted,
                ))
            })
            .unzip();
        value_gate::cpao::cost_per_accepted_outcome(&costs, &accepted)
    }
}

pub(crate) fn protocol_profile(profile: &TaskProfileLocal) -> TaskProfileV1 {
    TaskProfileV1 {
        primary_intent: profile.intent.clone(),
        task_class: profile.task_class.clone(),
        complexity: match profile.complexity.as_str() {
            "medium" => TaskComplexity::Medium,
            "high" => TaskComplexity::High,
            _ => TaskComplexity::Low,
        },
        scope: match profile.scope {
            TaskScopeLocal::SingleFile => TaskScope::SingleFile,
            TaskScopeLocal::MultiFile => TaskScope::MultiFile,
            TaskScopeLocal::CrossModule => TaskScope::CrossModule,
            TaskScopeLocal::CrossProject => TaskScope::CrossProject,
        },
        context_need_milli: profile.context_need_milli,
        reasoning_need_milli: profile.reasoning_need_milli,
        risk_signal: match profile.risk_signal_milli {
            750.. => RiskClass::High,
            400.. => RiskClass::Medium,
            _ => RiskClass::Low,
        },
        confidence_milli: profile.confidence_milli,
        capability_id: None,
        capability_version: None,
        keywords: Vec::new(),
        language_hints: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::value_gate::cost_tracker::calculate_cost;

    fn cost() -> ExecutionCost {
        ExecutionCost {
            input_tokens: 1_000,
            output_tokens: 500,
            cache_read_tokens: 0,
            model: "gpt-4o".into(),
            provider: "openai".into(),
            estimated_cost_micros: calculate_cost(1_000, 500, 0, "gpt-4o"),
        }
    }

    #[test]
    fn test_full_decision_loop() {
        let loop_ = DecisionLoop::default();
        let mut result = loop_.execute_task("fix bug in auth.rs", "session", "agent");
        loop_.complete_task(
            &mut result,
            cost(),
            vec![OutcomeSignal::BuildSucceeded, OutcomeSignal::TestsPassed],
        );

        assert_eq!(result.profile.intent, "coding_fix");
        assert!(result.envelope_created);
        assert_eq!(result.envelope.task_id.as_str(), result.task_id);
        assert_eq!(
            TaskSpine::task_id().as_deref(),
            Some(result.task_id.as_str())
        );
        assert_eq!(result.cost.as_ref().unwrap().input_tokens, 1_000);
        assert_eq!(result.cost.as_ref().unwrap().output_tokens, 500);
        assert_eq!(result.cost.as_ref().unwrap().model, "gpt-4o");
        assert!(result.assessment.unwrap().cpao_micros.is_some());
    }

    #[test]
    fn test_rejected_outcome() {
        let loop_ = DecisionLoop::default();
        let mut result = loop_.execute_task("fix bug in auth.rs", "session", "agent");
        loop_.complete_task(&mut result, cost(), vec![OutcomeSignal::TestFailed]);

        assert!(!result.assessment.as_ref().unwrap().outcome_accepted);
        assert_eq!(result.assessment.unwrap().cpao_micros, None);
    }

    #[test]
    fn test_unknown_query() {
        let result = DecisionLoop::default().execute_task("", "session", "agent");

        assert_eq!(result.profile.confidence_milli, 300);
        assert!(result.envelope_created);
    }

    #[test]
    fn test_multiple_tasks_cpao() {
        let loop_ = DecisionLoop::default();
        let mut results: Vec<_> = (0..3)
            .map(|index| {
                loop_.execute_task("fix bug in auth.rs", "session", &format!("agent-{index}"))
            })
            .collect();
        for result in &mut results {
            loop_.complete_task(result, cost(), vec![OutcomeSignal::BuildSucceeded]);
        }

        assert_eq!(
            DecisionLoop::aggregate_cpao(&results),
            Some(cost().estimated_cost_micros)
        );
    }
}
