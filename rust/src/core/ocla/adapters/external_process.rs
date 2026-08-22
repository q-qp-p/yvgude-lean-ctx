//! Local-process OCLA capability reference.
//!
//! The companion cookbook executable accepts literal UTF-8 on stdin. The
//! deterministic `LocalProcessAdapter` retains an in-process conformance
//! reference, while `ExternalProcessAdapter` discovers and invokes a fixed
//! local executable. Neither path interprets task input as a command.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use lean_ctx_protocol::{CapabilityManifestV1, DataMovement, Determinism};

use crate::core::ocla::invocation::{
    CAPABILITY_OBSERVATION_SCHEMA_VERSION, CapabilityAdapter, CapabilityInput,
    CapabilityInvocation, CapabilityObservationV1, CapabilityResult, check_timeout, evidence_ref,
};
use crate::core::ocla::{OclaError, OclaResult};

/// Maximum UTF-8 payload accepted by both the executable and host reference.
const MAX_INPUT_BYTES: usize = 64 * 1024;
/// The fixed JSON result remains well below this bound for every accepted input.
const MAX_OUTPUT_BYTES: usize = 128;
/// External reference processes must never receive an unbounded runtime.
const MAX_PROCESS_TIMEOUT_MS: u64 = 30_000;

const MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/ocla/capability-manifests/example/word-count-optimizer-v1.json"
));

fn manifest() -> &'static CapabilityManifestV1 {
    static MANIFEST_CACHE: OnceLock<CapabilityManifestV1> = OnceLock::new();

    MANIFEST_CACHE.get_or_init(|| {
        let manifest: CapabilityManifestV1 =
            serde_json::from_str(MANIFEST).expect("word-count optimizer manifest must be valid");
        manifest
            .validate()
            .expect("word-count optimizer manifest must satisfy the protocol");
        assert!(
            is_safe_local_process_contract(&manifest),
            "word-count optimizer manifest must stay local-only, bounded, and network-free"
        );
        manifest
    })
}

fn is_safe_local_process_contract(manifest: &CapabilityManifestV1) -> bool {
    manifest.capability_id.as_str() == "capability://example/word-count-optimizer"
        && manifest.version == "1.0.0"
        && manifest.local
        && !manifest.remote
        && manifest.determinism == Determinism::Deterministic
        && manifest.data_movement == DataMovement::LocalOnly
        && manifest
            .extra
            .get("execution")
            .and_then(serde_json::Value::as_str)
            == Some("local_process")
        && manifest
            .extra
            .get("entrypoint")
            .and_then(serde_json::Value::as_str)
            == Some("word-count-optimizer")
        && manifest
            .extra
            .get("network_access")
            .and_then(serde_json::Value::as_str)
            == Some("none")
        && manifest
            .extra
            .get("max_input_bytes")
            .and_then(serde_json::Value::as_u64)
            == Some(MAX_INPUT_BYTES as u64)
        && manifest
            .extra
            .get("max_output_bytes")
            .and_then(serde_json::Value::as_u64)
            == Some(MAX_OUTPUT_BYTES as u64)
}

/// Deterministic host reference for the cookbook's local executable.
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

        let CapabilityInput::ShellCommand { command, workdir } = &invocation.input else {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer requires literal stdin text".into(),
            ));
        };
        if workdir.is_some() {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer does not accept a working directory".into(),
            ));
        }
        if command.trim().is_empty() {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer input must not be empty".into(),
            ));
        }
        if command.len() > MAX_INPUT_BYTES {
            return Err(OclaError::InvalidRequest(format!(
                "input exceeds local-process byte limit ({} > {MAX_INPUT_BYTES})",
                command.len()
            )));
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
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(OclaError::InvalidRequest(
                "local-process output exceeds its declared byte limit".into(),
            ));
        }
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
        Ok(self.manifest().validate().is_ok() && is_safe_local_process_contract(self.manifest()))
    }
}

/// A discovered local executable implementing the word-count capability.
///
/// The executable and its arguments are operator configuration, never task
/// input. Invocation payloads flow exclusively over bounded UTF-8 stdin; no
/// shell, working directory, inherited environment, or network configuration
/// is supplied by LeanCTX.
pub(crate) struct ExternalProcessAdapter {
    manifest: CapabilityManifestV1,
    executable: PathBuf,
    arguments: Vec<OsString>,
    disabled: AtomicBool,
}

impl ExternalProcessAdapter {
    /// Discover and validate a local-process capability before registration.
    pub(crate) fn discover(
        manifest_path: impl AsRef<Path>,
        executable: impl AsRef<Path>,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> OclaResult<Self> {
        let manifest_path = manifest_path.as_ref();
        let manifest_bytes = fs::read(manifest_path).map_err(|error| {
            OclaError::InvalidRequest(format!(
                "cannot read external capability manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
        let manifest: CapabilityManifestV1 =
            serde_json::from_slice(&manifest_bytes).map_err(|error| {
                OclaError::InvalidRequest(format!("invalid external capability manifest: {error}"))
            })?;
        manifest.validate().map_err(|error| {
            OclaError::InvalidRequest(format!("invalid external capability manifest: {error}"))
        })?;
        if !is_safe_local_process_contract(&manifest) {
            return Err(OclaError::InvalidRequest(
                "external capability manifest is not the bounded local-process contract".into(),
            ));
        }

        let executable = executable.as_ref().canonicalize().map_err(|error| {
            OclaError::InvalidRequest(format!(
                "cannot resolve external capability executable {}: {error}",
                executable.as_ref().display()
            ))
        })?;
        if !executable.is_file() {
            return Err(OclaError::InvalidRequest(
                "external capability executable must be a regular file".into(),
            ));
        }

        Ok(Self {
            manifest,
            executable,
            arguments: arguments.into_iter().collect(),
            disabled: AtomicBool::new(false),
        })
    }

    /// Disable subsequent invocation without unregistering the manifest.
    pub(crate) fn disable(&self) {
        self.disabled.store(true, Ordering::Release);
    }

    fn validate_text_input<'a>(
        &self,
        invocation: &'a CapabilityInvocation,
    ) -> OclaResult<(&'a str, u64)> {
        invocation.validate()?;
        if invocation.capability_id != self.manifest.capability_id.as_str()
            || invocation.capability_version != self.manifest.version
        {
            return Err(OclaError::InvalidRequest(
                "invocation capability identity does not match adapter manifest".into(),
            ));
        }
        let CapabilityInput::ShellCommand { command, workdir } = &invocation.input else {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer requires literal stdin text".into(),
            ));
        };
        if workdir.is_some() || command.trim().is_empty() || command.len() > MAX_INPUT_BYTES {
            return Err(OclaError::InvalidRequest(
                "word-count optimizer input violates the bounded stdin contract".into(),
            ));
        }
        let input_tokens = command.split_whitespace().count() as u64;
        if invocation
            .policy_constraints
            .max_input_tokens
            .is_some_and(|maximum| input_tokens > maximum)
        {
            return Err(OclaError::InvalidRequest(
                "input exceeds policy token limit".into(),
            ));
        }
        if invocation.timeout_ms == 0 || invocation.timeout_ms > MAX_PROCESS_TIMEOUT_MS {
            return Err(OclaError::InvalidRequest(format!(
                "external capability timeout must be 1..={MAX_PROCESS_TIMEOUT_MS}ms"
            )));
        }
        Ok((command, input_tokens))
    }

    fn run(&self, input: &str, timeout_ms: u64) -> OclaResult<String> {
        let mut child = Command::new(&self.executable)
            .args(&self.arguments)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                OclaError::InvalidRequest(format!("cannot start external capability: {error}"))
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            OclaError::InvalidRequest("external capability stdin was unavailable".into())
        })?;
        stdin.write_all(input.as_bytes()).map_err(|error| {
            OclaError::InvalidRequest(format!("cannot write external capability input: {error}"))
        })?;
        drop(stdin);

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|error| {
                OclaError::InvalidRequest(format!("cannot observe external capability: {error}"))
            })? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OclaError::InvalidRequest(
                    "external capability exceeded its timeout".into(),
                ));
            }
            thread::sleep(Duration::from_millis(1));
        };

        let mut output = Vec::with_capacity(MAX_OUTPUT_BYTES);
        child
            .stdout
            .take()
            .ok_or_else(|| {
                OclaError::InvalidRequest("external capability stdout was unavailable".into())
            })?
            .take((MAX_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut output)
            .map_err(|error| {
                OclaError::InvalidRequest(format!(
                    "cannot read external capability output: {error}"
                ))
            })?;
        if !status.success() {
            return Err(OclaError::InvalidRequest(
                "external capability exited unsuccessfully".into(),
            ));
        }
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(OclaError::InvalidRequest(
                "external capability exceeded its declared output limit".into(),
            ));
        }
        String::from_utf8(output).map_err(|error| {
            OclaError::InvalidRequest(format!(
                "external capability returned non-UTF-8 output: {error}"
            ))
        })
    }
}

impl CapabilityAdapter for ExternalProcessAdapter {
    fn manifest(&self) -> &CapabilityManifestV1 {
        &self.manifest
    }

    fn invoke(&self, invocation: CapabilityInvocation) -> OclaResult<CapabilityResult> {
        if self.disabled.load(Ordering::Acquire) {
            return Err(OclaError::InvalidRequest(
                "external capability is disabled".into(),
            ));
        }
        let start = Instant::now();
        let (input, input_tokens) = self.validate_text_input(&invocation)?;
        let output = self.run(input, invocation.timeout_ms)?;
        let metrics = parse_metrics(&output)?;
        let expected = BTreeMap::from([
            ("word_count".into(), input_tokens),
            ("char_count".into(), input.chars().count() as u64),
            ("line_count".into(), input.lines().count() as u64),
        ]);
        if metrics != expected {
            return Err(OclaError::InvalidRequest(
                "external capability metrics do not match its input".into(),
            ));
        }
        let output_tokens = output.split_whitespace().count() as u64;
        let latency_ms = check_timeout(start, invocation.timeout_ms)?;
        if invocation
            .policy_constraints
            .max_latency_ms
            .is_some_and(|maximum| latency_ms > maximum)
            || invocation
                .policy_constraints
                .max_output_tokens
                .is_some_and(|maximum| output_tokens > maximum)
        {
            return Err(OclaError::InvalidRequest(
                "external capability exceeds a policy limit".into(),
            ));
        }
        let output_ref = evidence_ref(&output);
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
        Ok(!self.disabled.load(Ordering::Acquire)
            && self.executable.is_file()
            && self.manifest.validate().is_ok()
            && is_safe_local_process_contract(&self.manifest))
    }
}

fn parse_metrics(output: &str) -> OclaResult<BTreeMap<String, u64>> {
    let metrics: BTreeMap<String, u64> = serde_json::from_str(output.trim()).map_err(|error| {
        OclaError::InvalidRequest(format!(
            "external capability returned invalid JSON: {error}"
        ))
    })?;
    let expected_keys = ["char_count", "line_count", "word_count"];
    if metrics.len() != expected_keys.len()
        || expected_keys.iter().any(|key| !metrics.contains_key(*key))
    {
        return Err(OclaError::InvalidRequest(
            "external capability output does not satisfy its declared schema".into(),
        ));
    }
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ocla::adapters::registry::AdapterRegistry;
    use crate::core::ocla::invocation::PolicyConstraints;
    #[cfg(unix)]
    use tempfile::TempDir;

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
        assert!(is_safe_local_process_contract(m));
        assert!(adapter.health_check().expect("contract health check"));
    }

    #[test]
    fn invoke_word_count() {
        let adapter = LocalProcessAdapter;
        let result = adapter.invoke(test_invocation("hello world foo")).unwrap();
        assert!(result.success);
        assert_eq!(result.observation.input_tokens, 3);
        assert!(
            result
                .observation
                .output_ref
                .as_deref()
                .is_some_and(|reference| reference.starts_with("blake3:"))
        );
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
    fn output_reference_is_deterministic_and_payload_free() {
        let adapter = LocalProcessAdapter;
        let first = adapter.invoke(test_invocation("hello world")).unwrap();
        let second = adapter.invoke(test_invocation("hello world")).unwrap();

        assert_eq!(first.evidence_ref, second.evidence_ref);
        assert_eq!(first.observation.metrics, second.observation.metrics);
        assert!(
            first
                .evidence_ref
                .as_deref()
                .is_some_and(|reference| !reference.contains("hello"))
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
    fn rejects_workdir_and_oversized_input() {
        let adapter = LocalProcessAdapter;
        let mut with_workdir = test_invocation("hello");
        with_workdir.input = CapabilityInput::ShellCommand {
            command: "hello".into(),
            workdir: Some("/tmp".into()),
        };
        assert!(adapter.invoke(with_workdir).is_err());

        let oversized = "x".repeat(MAX_INPUT_BYTES + 1);
        assert!(adapter.invoke(test_invocation(&oversized)).is_err());
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

    #[test]
    fn manifest_passes_existing_conformance_check() {
        let result = crate::core::conformance::check_manifest_conformance(manifest());
        assert_eq!(result.checks_failed, 0, "{:?}", result.failures);
    }

    #[cfg(unix)]
    fn external_manifest(temp: &TempDir) -> PathBuf {
        let path = temp.path().join("manifest.json");
        fs::write(&path, MANIFEST).expect("fixture manifest write");
        path
    }

    #[cfg(unix)]
    fn printf_adapter(temp: &TempDir) -> ExternalProcessAdapter {
        ExternalProcessAdapter::discover(
            external_manifest(temp),
            "/usr/bin/printf",
            [OsString::from(
                "{\"word_count\":3,\"char_count\":15,\"line_count\":1}",
            )],
        )
        .expect("bounded local process should be discovered")
    }

    #[cfg(unix)]
    #[test]
    fn discovered_external_process_invokes_reports_and_registers() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let adapter = printf_adapter(&temp);
        let result = adapter
            .invoke(test_invocation("hello world foo"))
            .expect("external process invocation");

        assert!(result.success);
        assert_eq!(result.observation.metrics.get("word_count"), Some(&3));
        assert!(adapter.health_check().expect("external process health"));

        let registry = AdapterRegistry::new();
        registry.register(adapter).expect("external registration");
        assert!(
            registry
                .lookup("capability://example/word-count-optimizer", "1.0.0")
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_discovery_rejects_invalid_manifest_and_process_failure() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let invalid = temp.path().join("invalid.json");
        fs::write(&invalid, "{}").expect("invalid fixture write");
        assert!(ExternalProcessAdapter::discover(&invalid, "/usr/bin/printf", []).is_err());

        let adapter =
            ExternalProcessAdapter::discover(external_manifest(&temp), "/usr/bin/false", [])
                .expect("existing process can be discovered");
        assert!(adapter.invoke(test_invocation("hello world foo")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn external_adapter_can_be_disabled_without_unregistering() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let adapter = printf_adapter(&temp);
        adapter.disable();

        assert!(adapter.invoke(test_invocation("hello world foo")).is_err());
        assert!(!adapter.health_check().expect("disabled health check"));
    }
}
