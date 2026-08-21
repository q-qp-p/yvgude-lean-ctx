#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::suspicious,
    clippy::nursery,
    unused
)]
use super::*;
use crate::core::knowledge::KnowledgeQuery;

fn context() -> ContextState {
    ContextState::new(Vec::new(), KnowledgeQuery::default())
}

#[test]
fn test_advise_with_jira_ref() {
    let advice = KnowledgeGateAdvisor::advise("Fix LEAN-42", &context());
    assert_eq!(advice.references_found.len(), 1);
    assert_eq!(
        advice.references_found[0].ref_type,
        super::super::reference_resolver::ReferenceType::JiraIssue
    );
}

#[test]
fn test_advise_with_github_ref() {
    let advice = KnowledgeGateAdvisor::advise("Review #789", &context());
    assert_eq!(advice.references_found.len(), 1);
    assert_eq!(
        advice.references_found[0].ref_type,
        super::super::reference_resolver::ReferenceType::GitHubPR
    );
}

#[test]
fn test_advise_no_refs() {
    assert!(
        KnowledgeGateAdvisor::advise("Hello world", &context())
            .references_found
            .is_empty()
    );
}

#[test]
fn test_advise_multiple_refs() {
    assert_eq!(
        KnowledgeGateAdvisor::advise("Fix LEAN-42 and check #789", &context())
            .references_found
            .len(),
        2
    );
}

#[test]
fn test_hint_format() {
    assert!(
        KnowledgeGateAdvisor::advise("Fix LEAN-42", &context())
            .additional_context_hint
            .unwrap()
            .contains("Referenced:")
    );
}

#[derive(Debug)]
struct PanickingResolver;

impl ReferenceResolver for PanickingResolver {
    fn resolve(&self, _: &str) -> Vec<ResolvedReference> {
        panic!("resolver failed")
    }

    fn name(&self) -> &'static str {
        "panic"
    }
}

#[test]
fn test_error_tolerance() {
    assert!(
        KnowledgeGateAdvisor::advise_with("LEAN-42", &context(), &PanickingResolver)
            .references_found
            .is_empty()
    );
}
