use super::decision_loop_runtime::DecisionLoopRuntime;
use super::triage::{
    ProfileHypothesis, TaskAnalysisInput, TaskAnalyzer, TriageEngine, TriageError,
};

#[derive(Debug)]
struct FailingAnalyzer;

impl TaskAnalyzer for FailingAnalyzer {
    fn analyze(&self, _: &TaskAnalysisInput) -> Result<ProfileHypothesis, TriageError> {
        Err(TriageError::InternalError("test failure".to_owned()))
    }

    fn name(&self) -> &'static str {
        "failing"
    }
}

#[test]
fn test_runtime_init() {
    assert!(std::ptr::eq(
        DecisionLoopRuntime::get_or_init(),
        DecisionLoopRuntime::get_or_init()
    ));
}

#[test]
fn test_on_tool_start() {
    let context = DecisionLoopRuntime::get_or_init().on_tool_start(
        "ctx_read",
        Some("read lib.rs"),
        "runtime-test",
        "agent",
    );
    assert!(!context.task_id.is_empty());
    assert!(!context.profile_complexity.is_empty());
}

#[test]
fn test_on_tool_end_success() {
    let runtime = DecisionLoopRuntime::with_triage(TriageEngine::default());
    let context = runtime.on_tool_start("ctx_read", Some("read"), "runtime-test", "agent");
    runtime.on_tool_end(&context, 1, 1, "gpt-4o", true);
    assert_eq!(runtime.latest_assessment_accepted(), Some(true));
}

#[test]
fn test_on_tool_end_failure() {
    let runtime = DecisionLoopRuntime::with_triage(TriageEngine::default());
    let context = runtime.on_tool_start("ctx_read", Some("read"), "runtime-test", "agent");
    runtime.on_tool_end(&context, 1, 1, "gpt-4o", false);
    assert_eq!(runtime.latest_assessment_accepted(), Some(false));
}

#[test]
fn test_error_does_not_block() {
    let runtime =
        DecisionLoopRuntime::with_triage(TriageEngine::new(vec![Box::new(FailingAnalyzer)]));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.on_tool_start("ctx_read", None, "test", "agent")
        }))
        .is_ok()
    );
}

#[test]
fn missing_task_text_yields_passthrough_profile() {
    use crate::core::triage::confidence::ACTIONABLE_FLOOR_MILLI;
    use crate::server::context_gate::triage_filter_level;

    let runtime = DecisionLoopRuntime::with_triage(TriageEngine::default());
    let context = runtime.on_tool_start("ctx_read", None, "no-task-text", "agent");

    assert_eq!(context.profile_intent, "explore");
    let profile = runtime.profile_for_session("no-task-text").unwrap();
    assert!(profile.confidence_milli < ACTIONABLE_FLOOR_MILLI);
    assert_eq!(triage_filter_level(&profile), 0);
}
