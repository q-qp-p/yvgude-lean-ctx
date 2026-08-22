//! Status message for unavailable hosted Research work.
//!
//! The public Runtime is local-first. Hosted account, marketplace, and
//! organization operations must not be presented as purchasable product paths
//! before their status gates are met.

use crate::cloud_client;
use crate::core::billing::{Plan, min_plan_for};

/// Build the status text for a known hosted capability. Local and unknown
/// features intentionally produce no message.
fn render_hint(feature: &str, _current: Plan) -> Option<String> {
    min_plan_for(feature)?;
    Some(
        "\nThis hosted capability is Research and unavailable in the public LeanCTX Runtime.\n\
         Local context tooling continues to work without an account.\n"
            .to_string(),
    )
}

/// Print the current status for a known hosted Research capability.
pub(crate) fn hint_for(feature: &str) {
    let eff = cloud_client::resolve_effective_plan_cached();
    if let Some(text) = render_hint(feature, eff.plan) {
        print!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_feature_produces_no_hint() {
        assert!(render_hint("read", Plan::Free).is_none());
        assert!(render_hint("some_local_thing", Plan::Free).is_none());
    }

    #[test]
    fn hosted_capability_is_marked_research_and_reassures_local() {
        let text = render_hint("cloud_sync", Plan::Free).expect("gated → hint");
        assert!(text.contains("Research and unavailable"));
        assert!(text.contains("Local context tooling continues to work"));
    }

    #[test]
    fn hosted_capability_does_not_advertise_a_plan() {
        let text = render_hint("private_registry", Plan::Pro).expect("gated → hint");
        assert!(!text.contains("upgrade"));
        assert!(!text.contains("Enterprise"));
    }

    #[test]
    fn rendered_status_does_not_depend_on_plan() {
        let text = render_hint("cloud_sync", Plan::Pro).expect("still renders");
        assert_eq!(text, render_hint("cloud_sync", Plan::Free).unwrap());
    }

    #[test]
    fn enterprise_capability_is_research() {
        let text = render_hint("sso_scim", Plan::Team).expect("gated → hint");
        assert!(text.contains("Research and unavailable"));
    }

    #[test]
    fn oidc_sso_is_research() {
        let text = render_hint("sso_oidc", Plan::Pro).expect("gated");
        assert!(text.contains("Research and unavailable"));
    }
}
