//! Native LeanCTX context optimization adapter.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use lean_ctx_protocol::CapabilityManifestV1;

use super::read_context_paths;
use crate::core::compressor;
use crate::core::ocla::invocation::{
    CapabilityAdapter, CapabilityInput, CapabilityInvocation, CapabilityObservationV1,
    CapabilityResult, evidence_ref,
};
use crate::core::ocla::{OclaError, OclaResult};
use crate::core::tokens;

/// Repository-relative manifest used by this adapter.
pub const MANIFEST_PATH: &str =
    "docs/contracts/ocla/capability-manifests/leanctx/context-optimization-v1.json";
const MANIFEST_JSON: &str = include_str!(
    "../../../../../docs/contracts/ocla/capability-manifests/leanctx/context-optimization-v1.json"
);

fn manifest() -> &'static CapabilityManifestV1 {
    static MANIFEST: OnceLock<CapabilityManifestV1> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest: CapabilityManifestV1 =
            serde_json::from_str(MANIFEST_JSON).expect("native context manifest is valid JSON");
        manifest
            .validate()
            .expect("native context manifest satisfies the protocol");
        manifest
    })
}

/// Local adapter that combines existing file reads and compression primitives.
pub struct NativeContextAdapter {
    root: PathBuf,
}

impl NativeContextAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    #[must_use]
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn optimize(&self, content: &str, paths: &[String], mode: &str) -> String {
        if matches!(mode, "full" | "raw" | "passthrough") {
            return content.to_owned();
        }

        let extension = paths
            .first()
            .and_then(|path| Path::new(path).extension())
            .and_then(|extension| extension.to_str());
        compressor::aggressive_compress(content, extension)
    }

    fn invoke_context(
        &self,
        invocation: &CapabilityInvocation,
        paths: &[String],
        mode: &str,
        budget_tokens: Option<u64>,
        start: Instant,
    ) -> OclaResult<CapabilityResult> {
        let input = read_context_paths(&self.root, paths)?;
        let input_tokens = tokens::count_tokens(&input) as u64;
        if let Some(max) = invocation.policy_constraints.max_input_tokens
            && input_tokens > max
        {
            return Err(OclaError::InvalidRequest(format!(
                "input exceeds policy token limit ({input_tokens} > {max})"
            )));
        }

        let optimized = self.optimize(&input, paths, mode);
        let output = match budget_tokens {
            Some(budget) => truncate_to_tokens(&optimized, budget),
            None => optimized,
        };
        let output_tokens = tokens::count_tokens(&output) as u64;
        if let Some(max) = invocation.policy_constraints.max_output_tokens
            && output_tokens > max
        {
            return Err(OclaError::InvalidRequest(format!(
                "output exceeds policy token limit ({output_tokens} > {max})"
            )));
        }

        let latency_ms = super::super::invocation::check_timeout(start, invocation.timeout_ms)?;
        if let Some(max) = invocation.policy_constraints.max_latency_ms
            && latency_ms > max
        {
            return Err(OclaError::InvalidRequest(format!(
                "capability latency exceeds policy limit ({latency_ms} > {max})"
            )));
        }
        let output_ref = evidence_ref(&output);
        let mut observation = CapabilityObservationV1::success(
            invocation,
            input_tokens,
            output_tokens,
            latency_ms,
            Some(output_ref.clone()),
        );
        observation.metrics.insert(
            "compression_saved_tokens".into(),
            input_tokens.saturating_sub(output_tokens),
        );
        observation.metrics.insert(
            "compression_rate_milli".into(),
            compression_rate_milli(input_tokens, output_tokens),
        );
        Ok(CapabilityResult {
            success: true,
            output_tokens,
            latency_ms,
            observation,
            evidence_ref: Some(output_ref),
        })
    }
}

impl Default for NativeContextAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityAdapter for NativeContextAdapter {
    fn manifest(&self) -> &CapabilityManifestV1 {
        manifest()
    }

    fn invoke(&self, invocation: CapabilityInvocation) -> OclaResult<CapabilityResult> {
        invocation.validate()?;
        if invocation.capability_id != self.manifest().capability_id.as_str()
            || invocation.capability_version != self.manifest().version
        {
            return Err(OclaError::InvalidRequest(
                "invocation capability identity does not match adapter manifest".into(),
            ));
        }
        let start = Instant::now();
        match invocation.input.clone() {
            CapabilityInput::ContextRequest {
                paths,
                mode,
                budget_tokens,
            } => self.invoke_context(&invocation, &paths, &mode, budget_tokens, start),
            CapabilityInput::ShellCommand { .. } | CapabilityInput::ModelRequest { .. } => {
                Err(OclaError::InvalidRequest(
                    "native context optimization accepts ContextRequest only".into(),
                ))
            }
        }
    }

    fn health_check(&self) -> OclaResult<bool> {
        Ok(self.root.is_dir() && self.manifest().validate().is_ok())
    }
}

fn truncate_to_tokens(content: &str, budget: u64) -> String {
    if budget == 0 {
        return String::new();
    }
    if tokens::count_tokens(content) as u64 <= budget {
        return content.to_owned();
    }
    content
        .split_whitespace()
        .take(budget as usize)
        .collect::<Vec<_>>()
        .join(" ")
}

fn compression_rate_milli(input_tokens: u64, output_tokens: u64) -> u64 {
    if input_tokens == 0 {
        return 0;
    }
    input_tokens
        .saturating_sub(output_tokens)
        .saturating_mul(1000)
        .checked_div(input_tokens)
        .unwrap_or(0)
        .min(1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ocla::invocation::{CapabilityAdapter, PolicyConstraints};

    #[test]
    fn manifest_is_pinned_to_native_path() {
        assert!(MANIFEST_PATH.ends_with("context-optimization-v1.json"));
        assert_eq!(manifest().provider, "leanctx");
    }

    #[test]
    fn rejects_non_context_inputs() {
        let adapter = NativeContextAdapter::new();
        let invocation = CapabilityInvocation {
            task_id: "task-1".into(),
            capability_id: adapter.manifest().capability_id.as_str().into(),
            capability_version: adapter.manifest().version.clone(),
            input: CapabilityInput::ShellCommand {
                command: "printf test".into(),
                workdir: None,
            },
            policy_constraints: PolicyConstraints::default(),
            timeout_ms: 100,
        };
        assert!(adapter.invoke(invocation).is_err());
    }
}
