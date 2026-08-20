use super::{
    ProfileHypothesis, TaskAnalysisInput, TaskAnalyzer, TriageBackendLocal,
    confidence::{RULES_FALLBACK_MILLI, clamp_milli},
    profile::{TaskProfileLocal, TaskScopeLocal},
};
use crate::core::{
    adaptive::TaskComplexity,
    intent_engine::{self, IntentScope, StructuredIntent, TaskType},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleTriageBackend;

impl TaskAnalyzer for RuleTriageBackend {
    fn analyze(&self, input: &TaskAnalysisInput) -> Result<ProfileHypothesis, super::TriageError> {
        if input.query.trim().is_empty() {
            return Ok(fallback());
        }
        let classification = intent_engine::classify(&input.query);
        let structured =
            StructuredIntent::from_query_with_session(&input.query, &input.files_touched);
        let (complexity, reasoning) = complexity_values(intent_engine::classify_complexity(
            &input.query,
            &classification,
        ));
        let confidence = milli(structured.confidence);
        let (scope, scope_base) = scope_values(structured.scope);
        let profile = TaskProfileLocal {
            task_class: "coding".into(),
            intent: intent_name(structured.task_type).into(),
            complexity: complexity.into(),
            scope,
            context_need_milli: clamp_milli(
                scope_base
                    + input.files_touched.len().min(6) as u16 * 25
                    + u16::from(input.session_context.is_some()) * 50,
            ),
            reasoning_need_milli: reasoning,
            risk_signal_milli: clamp_milli(
                input.active_diagnostics.saturating_mul(100).min(600) as u16
                    + u16::from(matches!(
                        structured.task_type,
                        TaskType::Config | TaskType::Deploy
                    )) * 200,
            ),
            confidence_milli: confidence,
        };
        Ok(ProfileHypothesis {
            profile,
            confidence_milli: confidence,
            backend: TriageBackendLocal::Rules,
        })
    }
    fn name(&self) -> &'static str {
        "rules"
    }
}

fn fallback() -> ProfileHypothesis {
    let profile = TaskProfileLocal {
        task_class: "coding".into(),
        intent: "explore".into(),
        confidence_milli: RULES_FALLBACK_MILLI,
        ..Default::default()
    };
    ProfileHypothesis {
        profile,
        confidence_milli: RULES_FALLBACK_MILLI,
        backend: TriageBackendLocal::Rules,
    }
}
fn milli(value: f64) -> u16 {
    if value.is_finite() {
        clamp_milli((value.clamp(0.0, 1.0) * 1000.0).round() as u16)
    } else {
        // A non-finite confidence is an absent signal, not a middling one — it
        // reports the same guess-level confidence as `fallback()` so it stays
        // below `ACTIONABLE_FLOOR_MILLI` (#1484).
        RULES_FALLBACK_MILLI
    }
}
fn intent_name(t: TaskType) -> &'static str {
    match t {
        TaskType::FixBug => "coding_fix",
        _ => t.as_str(),
    }
}
fn complexity_values(c: TaskComplexity) -> (&'static str, u16) {
    match c {
        TaskComplexity::Mechanical => ("low", 250),
        TaskComplexity::Standard => ("medium", 550),
        TaskComplexity::Architectural => ("high", 850),
    }
}
fn scope_values(s: IntentScope) -> (TaskScopeLocal, u16) {
    match s {
        IntentScope::SingleFile => (TaskScopeLocal::SingleFile, 250),
        IntentScope::MultiFile => (TaskScopeLocal::MultiFile, 500),
        IntentScope::CrossModule => (TaskScopeLocal::CrossModule, 750),
        IntentScope::ProjectWide => (TaskScopeLocal::CrossProject, 900),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn intent(query: &str) -> String {
        RuleTriageBackend
            .analyze(&TaskAnalysisInput {
                query: query.into(),
                ..Default::default()
            })
            .unwrap()
            .profile
            .intent
    }
    #[test]
    fn maps_all_intents() {
        for (query, expected) in [
            ("generate code", "generate"),
            ("fix bug", "coding_fix"),
            ("refactor", "refactor"),
            ("explain", "explore"),
            ("test", "test"),
            ("debug", "debug"),
            ("configure", "config"),
            ("deploy", "deploy"),
            ("review", "review"),
        ] {
            assert_eq!(intent(query), expected);
        }
    }
    #[test]
    fn fallback_has_low_confidence() {
        assert_eq!(
            RuleTriageBackend
                .analyze(&TaskAnalysisInput::default())
                .unwrap()
                .confidence_milli,
            RULES_FALLBACK_MILLI
        );
    }
    #[test]
    fn non_finite_confidence_stays_below_the_actionable_floor() {
        use crate::core::triage::confidence::ACTIONABLE_FLOOR_MILLI;

        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                milli(value) < ACTIONABLE_FLOOR_MILLI,
                "a non-finite confidence is an absent signal, not an actionable one"
            );
        }
    }
}
