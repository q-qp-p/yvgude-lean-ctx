//! Production `ctx_read` bridge for the first receipt-backed Engine operation.

use std::sync::{Arc, Mutex};

use lean_ctx_protocol::{
    EngineObservationStatusV1, EnginePolicyAdmissionV1, EnginePolicyDecisionV1, ProtocolReference,
};
use rmcp::ErrorData;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::core::engine_interface::NativeContextEngine;
use crate::server::context_gate::PreDispatchResult;

#[derive(Clone, Default)]
pub(super) struct SourceSnapshot(Arc<Mutex<Option<String>>>);

impl SourceSnapshot {
    pub(super) fn capture(&self, enabled: bool, mode: &str, input: &str) {
        if enabled && mode == "aggressive" {
            let mut snapshot = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *snapshot = Some(input.to_owned());
        }
    }

    pub(super) fn record(
        self,
        project_root: &str,
        path: &str,
        policy_admission: EnginePolicyAdmissionV1,
    ) -> Result<(), String> {
        let snapshot = self
            .0
            .lock()
            .map_err(|_| "Engine source snapshot lock poisoned".to_owned())?
            .take();
        record_aggressive_snapshot(snapshot, project_root, path, policy_admission)
    }

    pub(super) fn record_if_enabled(
        self,
        project_root: &str,
        path: &str,
        policy_admission: Option<EnginePolicyAdmissionV1>,
    ) -> Option<String> {
        policy_admission.and_then(|admission| {
            self.record(project_root, path, admission)
                .err()
                .map(|error| stable_warning(&error))
        })
    }
}

pub(super) fn interface_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["v1"],
        "description": "Opt into one receipt-backed Engine v1 invocation (single-path aggressive reads only)"
    })
}

pub(super) fn interface_v1_requested(args: &Map<String, Value>) -> Result<bool, ErrorData> {
    match args.get("engine_interface") {
        None => Ok(false),
        Some(Value::String(version)) if version == "v1" => Ok(true),
        Some(_) => Err(ErrorData::invalid_params(
            "engine_interface must be the string \"v1\" when provided",
            None,
        )),
    }
}

pub(super) fn validate_v1_request_shape(
    args: &Map<String, Value>,
    enabled: bool,
) -> Result<(), ErrorData> {
    if !enabled {
        return Ok(());
    }
    if args.contains_key("paths") {
        return Err(ErrorData::invalid_params(
            "engine_interface=\"v1\" supports only single-path ctx_read",
            None,
        ));
    }
    if args.get("mode").and_then(Value::as_str) != Some("aggressive") {
        return Err(ErrorData::invalid_params(
            "engine_interface=\"v1\" requires mode=\"aggressive\"",
            None,
        ));
    }
    for unsupported in [
        "raw",
        "start_line",
        "offset",
        "limit",
        "aggressiveness",
        "protect",
    ] {
        if args.contains_key(unsupported) {
            return Err(ErrorData::invalid_params(
                format!("engine_interface=\"v1\" does not support the {unsupported} parameter"),
                None,
            ));
        }
    }
    Ok(())
}

pub(super) fn require_aggressive(enabled: bool, mode: &str) -> Result<(), ErrorData> {
    if enabled && mode != "aggressive" {
        return Err(ErrorData::invalid_params(
            "engine_interface=\"v1\" requires effective mode=\"aggressive\"",
            None,
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct GateAdmissionIdentityV1<'a> {
    schema_version: u32,
    requested_mode: &'a str,
    overridden_mode: Option<&'a str>,
    reason: Option<&'a str>,
    pressure_downgraded: bool,
    budget_blocked: bool,
    triage_filter_level: u8,
}

/// Convert the actual pre-dispatch decision into the Engine admission record.
pub(super) fn admission_from_gate(
    gate: &PreDispatchResult,
    requested_mode: &str,
) -> Result<EnginePolicyAdmissionV1, String> {
    let identity = GateAdmissionIdentityV1 {
        schema_version: 1,
        requested_mode,
        overridden_mode: gate.overridden_mode.as_deref(),
        reason: gate.reason,
        pressure_downgraded: gate.pressure_downgraded,
        budget_blocked: gate.budget_blocked,
        triage_filter_level: gate.triage_filter_level,
    };
    let bytes = crate::core::canonical::canonical_serialize(&identity);
    let digest = crate::core::agent_identity::hex_encode(&Sha256::digest(bytes));
    Ok(EnginePolicyAdmissionV1 {
        policy_ref: ProtocolReference::new(format!(
            "policy:ctx-read-context-gate-v1:sha256:{digest}"
        ))
        .map_err(|error| error.to_string())?,
        decision: if gate.budget_blocked
            || gate.pressure_downgraded
            || gate.overridden_mode.is_some()
            || gate.triage_filter_level > 0
        {
            EnginePolicyDecisionV1::Rejected
        } else {
            EnginePolicyDecisionV1::Admitted
        },
    })
}

pub(super) fn admission_or_reject(
    enabled: bool,
    gate: &PreDispatchResult,
    requested_mode: &str,
    project_root: &str,
    path: &str,
) -> Result<Option<EnginePolicyAdmissionV1>, ErrorData> {
    if !enabled {
        if gate.budget_blocked {
            return Err(ErrorData::invalid_params(
                gate.budget_warning
                    .clone()
                    .unwrap_or_else(|| "Agent token budget exceeded".to_owned()),
                None,
            ));
        }
        return Ok(None);
    }
    let admission = admission_from_gate(gate, requested_mode)
        .map_err(|error| ErrorData::internal_error(error, None))?;
    if admission.decision == EnginePolicyDecisionV1::Rejected {
        let receipt_ref = record_policy_rejection(project_root, path, admission)
            .map_err(|error| ErrorData::internal_error(stable_warning(&error), None))?;
        return Err(ErrorData::invalid_params(
            format!("engine_interface=\"v1\" rejected by context gate; receipt_ref={receipt_ref}"),
            None,
        ));
    }
    Ok(Some(admission))
}

/// Keep user-visible fallback deterministic and free of OS/path error text.
pub(super) fn stable_warning(error: &str) -> String {
    let mut warning = "[ENGINE RECEIPT WARNING] code=engine_record_unavailable".to_owned();
    for key in ["receipt_ref", "recovery_ref"] {
        let marker = format!("{key}=");
        let Some(value) = error
            .split_ascii_whitespace()
            .find_map(|token| token.strip_prefix(&marker))
            .map(|value| value.trim_end_matches([';', ',']))
        else {
            continue;
        };
        if ProtocolReference::new(value.to_owned()).is_ok() {
            warning.push(' ');
            warning.push_str(key);
            warning.push('=');
            warning.push_str(value);
        }
    }
    warning
}

/// Record the exact input snapshot captured by the cold read worker.
/// Omitted Engine calls may hit legacy cache; explicit v1 calls force fresh.
pub(super) fn record_aggressive_snapshot(
    snapshot: Option<String>,
    project_root: &str,
    path: &str,
    policy_admission: EnginePolicyAdmissionV1,
) -> Result<(), String> {
    let input = snapshot.ok_or_else(|| "Engine v1 source snapshot unavailable".to_owned())?;
    record_aggressive_invocation(project_root, path, &input, policy_admission)
}

pub(super) fn record_policy_rejection(
    project_root: &str,
    path: &str,
    policy_admission: EnginePolicyAdmissionV1,
) -> Result<String, String> {
    let engine = NativeContextEngine::with_root(project_root);
    let (_, observation) = engine.execute_ctx_read_rejection(path, policy_admission)?;
    if observation.status != EngineObservationStatusV1::Rejected {
        return Err("native Engine policy rejection returned a non-rejected status".to_owned());
    }
    observation
        .receipt_link
        .map(|link| link.receipt_ref.as_str().to_owned())
        .ok_or_else(|| "native Engine policy rejection omitted its receipt link".to_owned())
}

/// Record the native aggressive-compression capability against the source
/// snapshot already acquired by `ctx_read`; Engine applies active redaction.
fn record_aggressive_invocation(
    project_root: &str,
    path: &str,
    input: &str,
    policy_admission: EnginePolicyAdmissionV1,
) -> Result<(), String> {
    let engine = NativeContextEngine::with_root(project_root);
    let (_, observation) = engine.execute_ctx_read_snapshot(path, input, policy_admission)?;
    let receipt_link = observation
        .receipt_link
        .ok_or_else(|| "native Engine terminal observation omitted its receipt link".to_owned())?;
    if observation.status != EngineObservationStatusV1::Succeeded {
        let recovery_ref = observation
            .failure
            .as_ref()
            .and_then(|failure| failure.recovery_ref.as_ref())
            .map_or("none", ProtocolReference::as_str);
        return Err(format!(
            "Engine status={:?}; receipt_ref={}; recovery_ref={recovery_ref}",
            observation.status,
            receipt_link.receipt_ref.as_str()
        ));
    }
    tracing::debug!(
        status = ?observation.status,
        receipt_ref = receipt_link.receipt_ref.as_str(),
        "ctx_read Engine observation recorded"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(blocked: bool, overridden_mode: Option<&str>) -> PreDispatchResult {
        PreDispatchResult {
            overridden_mode: overridden_mode.map(str::to_owned),
            reason: overridden_mode.map(|_| "fixture-override"),
            pressure_downgraded: overridden_mode.is_some(),
            budget_blocked: blocked,
            budget_warning: None,
            triage_filter_level: u8::from(overridden_mode.is_some()),
        }
    }

    #[test]
    fn gate_admission_preserves_decision_and_exact_policy_identity() {
        let admitted = admission_from_gate(&gate(false, None), "aggressive").unwrap();
        let rejected = admission_from_gate(&gate(true, None), "aggressive").unwrap();
        let overridden =
            admission_from_gate(&gate(false, Some("signatures")), "aggressive").unwrap();

        assert_eq!(admitted.decision, EnginePolicyDecisionV1::Admitted);
        assert_eq!(rejected.decision, EnginePolicyDecisionV1::Rejected);
        assert_eq!(overridden.decision, EnginePolicyDecisionV1::Rejected);
        assert_ne!(admitted.policy_ref, rejected.policy_ref);
        assert_ne!(admitted.policy_ref, overridden.policy_ref);
        assert!(
            admitted
                .policy_ref
                .as_str()
                .starts_with("policy:ctx-read-context-gate-v1:sha256:")
        );
    }

    #[test]
    fn rejected_gate_persists_a_receipt_without_native_output() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rejected.rs");
        std::fs::write(&file, "fn must_not_execute() {}").unwrap();
        let error = admission_or_reject(
            true,
            &gate(true, None),
            "aggressive",
            &dir.path().to_string_lossy(),
            &file.to_string_lossy(),
        )
        .unwrap_err();
        assert!(error.message.contains("receipt_ref=receipt:sha256:"));
        let data_dir = crate::core::data_dir::lean_ctx_data_dir().unwrap();
        assert!(!data_dir.join("engine-interface/v1/outputs").exists());
        let receipt = std::fs::read_dir(data_dir.join("engine-interface/v1/receipts"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(receipt).unwrap()).unwrap();
        assert_eq!(receipt["observation"]["status"], "rejected");
        assert_eq!(
            receipt["invocation"]["policy_admission"]["decision"],
            "rejected"
        );
    }

    #[test]
    fn enabled_recording_never_silently_accepts_a_missing_snapshot() {
        let admission = admission_from_gate(&gate(false, None), "aggressive").unwrap();
        let error =
            record_aggressive_snapshot(None, "/tmp", "/tmp/missing", admission).unwrap_err();
        assert_eq!(error, "Engine v1 source snapshot unavailable");
    }

    #[test]
    fn warning_redacts_nondeterministic_storage_error_details() {
        let warning = stable_warning(
            "persist Engine receipt: /tmp/private: errno 13; recovery_ref=recovery:sha256:abc",
        );
        assert_eq!(
            warning,
            "[ENGINE RECEIPT WARNING] code=engine_record_unavailable recovery_ref=recovery:sha256:abc"
        );
        assert!(!warning.contains("/tmp/private"));
        assert!(!warning.contains("errno"));
    }

    #[test]
    fn engine_v1_rejects_non_aggressive_effective_mode() {
        require_aggressive(true, "aggressive").unwrap();
        let error = require_aggressive(true, "full").unwrap_err();
        assert_eq!(
            error.message,
            "engine_interface=\"v1\" requires effective mode=\"aggressive\""
        );
        require_aggressive(false, "full").unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn production_bridge_uses_cache_snapshot_without_second_disk_read() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("snapshot.rs");
        let cached = "fn cache_snapshot_marker() {}\n".repeat(40);
        std::fs::write(&file, "fn changed_disk_marker() {}\n").unwrap();
        let path = file.to_string_lossy().into_owned();
        let admission = admission_from_gate(&gate(false, None), "aggressive").unwrap();

        record_aggressive_snapshot(
            Some(cached),
            &dir.path().to_string_lossy(),
            &path,
            admission,
        )
        .unwrap();

        let output_dir = crate::core::data_dir::lean_ctx_data_dir()
            .unwrap()
            .join("engine-interface/v1/outputs");
        let output_path = std::fs::read_dir(output_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let output = std::fs::read_to_string(output_path).unwrap();
        assert!(output.contains("cache_snapshot_marker"));
        assert!(!output.contains("changed_disk_marker"));
    }
}
