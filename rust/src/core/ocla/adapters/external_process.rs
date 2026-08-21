use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;

use lean_ctx_protocol::CapabilityManifestV1;

use crate::core::ocla::invocation::{
    CAPABILITY_OBSERVATION_SCHEMA_VERSION, CapabilityAdapter, CapabilityInput,
    CapabilityInvocation, CapabilityObservationV1, CapabilityResult, check_timeout, evidence_ref,
};
use crate::core::ocla::{OclaError, OclaResult};

#[allow(dead_code)]
const MANIFEST: &str = include_str!(
    "../../../../../docs/contracts/ocla/capability-manifests/example/word-count-optimizer-v1.json"
);

#[allow(dead_code)]
fn manifest() -> &'static CapabilityManifestV1 {
    static MANIFEST_CACHE: OnceLock<CapabilityManifestV1> = OnceLock::new();

    MANIFEST_CACHE.get_or_init(|| {
        serde_json::from_str(MANIFEST).expect("word-count optimizer manifest must be valid")
    })
}

#[allow(dead_code)]
#[derive(Default)]
pub(crate) struct LocalProcessAdapter;

impl CapabilityAdapter for LocalProcessAdapter {
    fn manifest(&self) -> &'static CapabilityManifestV1 {
        manifest()
    }

    fn invoke(&self, invocation: CapabilityInvocation) -> OclaResult<CapabilityResult> {
        let start = Instant::now();
        invocation.validate()?;

        let manifest = self.manifest();
        if invocation.capability_id != manifest.capability_id.as_str()
            || invocation.capability_version != manifest.version
        {
            return Err(OclaError::InvalidRequest(
                "invocation capability identity does not match adapter manifest".into(),
            ));
        }

        let CapabilityInput::ShellCommand { command, .. } = &invocation.input else {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer requires a shell command input".into(),
            ));
        };
        if command.trim().is_empty() {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer input must not be empty".into(),
            ));
        }

        let input_tokens = command.split_whitespace().count() as u64;
        if let Some(max_input_tokens) = invocation.policy_constraints.max_input_tokens
            && input_tokens > max_input_tokens
        {
            return Err(OclaError::InvalidRequest(format!(
                "input exceeds policy token limit ({input_tokens} > {max_input_tokens})"
            )));
        }

        let word_count = input_tokens;
        let char_count = command.chars().count() as u64;
        let line_count = command.lines().count() as u64;
        let output = format!(
            "{{\"word_count\":{word_count},\"char_count\":{char_count},\"line_count\":{line_count}}}"
        );
        let output_tokens = output.split_whitespace().count() as u64;
        let latency_ms = check_timeout(start, invocation.timeout_ms)?;

        if let Some(max_latency_ms) = invocation.policy_constraints.max_latency_ms
            && latency_ms > max_latency_ms
        {
            return Err(OclaError::InvalidRequest(format!(
                "capability latency exceeds policy limit ({latency_ms}ms > {max_latency_ms}ms)"
            )));
        }
        if let Some(max_output_tokens) = invocation.policy_constraints.max_output_tokens
            && output_tokens > max_output_tokens
        {
            return Err(OclaError::InvalidRequest(format!(
                "output exceeds policy token limit ({output_tokens} > {max_output_tokens})"
            )));
        }

        let output_ref = evidence_ref(&output);
        let metrics = BTreeMap::from([
            ("word_count".into(), word_count),
            ("char_count".into(), char_count),
            ("line_count".into(), line_count),
        ]);

        Ok(CapabilityResult {
            success: true,
            output_tokens,
            latency_ms,
            observation: CapabilityObservationV1 {
                schema_version: CAPABILITY_OBSERVATION_SCHEMA_VERSION,
                task_id: invocation.task_id,
                success: true,
                capability_id: invocation.capability_id,
                capability_version: invocation.capability_version,
                input_tokens,
                output_tokens,
                latency_ms,
                failure_mode: None,
                output_ref: Some(output_ref.clone()),
                metrics,
            },
            evidence_ref: Some(output_ref),
        })
    }

    fn health_check(&self) -> OclaResult<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ocla::adapters::registry::AdapterRegistry;
    use crate::core::ocla::invocation::PolicyConstraints;

    fn test_invocation(text: &str) -> CapabilityInvocation {
        CapabilityInvocation {
            task_id: "test-task-1".into(),
            capability_id: "capability://example/word-count-optimizer".into(),
            capability_version: "1.0.0".into(),
            input: CapabilityInput::ShellCommand {
                command: text.into(),
                workdir: None,
            },
            policy_constraints: PolicyConstraints::default(),
            timeout_ms: 30_000,
        }
    }

    #[test]
    fn manifest_valid() {
        let adapter = LocalProcessAdapter;
        let m = adapter.manifest();
        assert_eq!(
            m.capability_id.as_str(),
            "capability://example/word-count-optimizer"
        );
        assert_eq!(m.version, "1.0.0");
        m.validate().expect("manifest must validate");
    }

    #[test]
    fn invoke_word_count() {
        let adapter = LocalProcessAdapter;
        let result = adapter.invoke(test_invocation("hello world foo")).unwrap();
        assert!(result.success);
        assert_eq!(result.observation.input_tokens, 3);
        let _parsed: serde_json::Value =
            serde_json::from_str(&result.observation.output_ref.as_deref().unwrap_or(""))
                .unwrap_or_default();
        assert_eq!(
            result.observation.metrics.get("word_count").copied(),
            Some(3)
        );
        assert_eq!(
            result.observation.metrics.get("line_count").copied(),
            Some(1)
        );
    }

    #[test]
    fn invoke_empty_input() {
        let adapter = LocalProcessAdapter;
        let inv = CapabilityInvocation {
            task_id: "test-empty".into(),
            capability_id: "capability://example/word-count-optimizer".into(),
            capability_version: "1.0.0".into(),
            input: CapabilityInput::ShellCommand {
                command: String::new(),
                workdir: None,
            },
            policy_constraints: PolicyConstraints::default(),
            timeout_ms: 30_000,
        };
        let result = adapter.invoke(inv);
        assert!(result.is_err());
    }

    #[test]
    fn invoke_multiline() {
        let adapter = LocalProcessAdapter;
        let text = "line one\nline two\nline three";
        let result = adapter.invoke(test_invocation(text)).unwrap();
        assert!(result.success);
        assert_eq!(
            result.observation.metrics.get("line_count").copied(),
            Some(3)
        );
        assert_eq!(
            result.observation.metrics.get("word_count").copied(),
            Some(6)
        );
    }

    #[test]
    fn invoke_wrong_capability_id() {
        let adapter = LocalProcessAdapter;
        let inv = CapabilityInvocation {
            task_id: "test-wrong-id".into(),
            capability_id: "capability://wrong/id".into(),
            capability_version: "1.0.0".into(),
            input: CapabilityInput::ShellCommand {
                command: "hello".into(),
                workdir: None,
            },
            policy_constraints: PolicyConstraints::default(),
            timeout_ms: 30_000,
        };
        assert!(adapter.invoke(inv).is_err());
    }

    #[test]
    fn invoke_wrong_version() {
        let adapter = LocalProcessAdapter;
        let inv = CapabilityInvocation {
            task_id: "test-wrong-ver".into(),
            capability_id: "capability://example/word-count-optimizer".into(),
            capability_version: "99.0.0".into(),
            input: CapabilityInput::ShellCommand {
                command: "hello".into(),
                workdir: None,
            },
            policy_constraints: PolicyConstraints::default(),
            timeout_ms: 30_000,
        };
        assert!(adapter.invoke(inv).is_err());
    }

    #[test]
    fn register_in_adapter_registry() {
        let registry = AdapterRegistry::new();
        registry.register(LocalProcessAdapter).unwrap();
        let resolved = registry.lookup("capability://example/word-count-optimizer", "1.0.0");
        assert!(resolved.is_some());
        let adapter = resolved.unwrap();
        assert_eq!(adapter.manifest().version, "1.0.0");
    }
}
