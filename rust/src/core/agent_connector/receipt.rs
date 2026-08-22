//! Signed receipt linkage for provider-backed connector executions.
//!
//! The connector CLI output is not itself a signed provider receipt.  This
//! module therefore emits a locally signed `ExecutionReceiptV1` only when its
//! JSON output explicitly reports both token usage and a cost.  Parsed source
//! evidence remains `Unverified`; the local signature only makes the recorded
//! observation tamper-evident and verifiable without network access.

use std::fs;
use std::path::PathBuf;

use lean_ctx_protocol::{
    ContextBalanceV1, EvidenceKind, EvidenceRefV1, ExecutionReceiptV1, PlanId, ReceiptId,
    SignatureStatus, TaskId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::traits::{TaskRequest, TokenUsage};
use crate::core::agent_identity::{
    current_agent_id, get_or_create_keypair, hex_decode, hex_encode,
};
use crate::core::canonical::{canonical_serialize, sign_receipt, verify_receipt_signature};
use crate::core::context_kernel::provider_normalization::{
    NormalizedUsage, decimal_value_to_micros, normalize_anthropic, normalize_openai,
};

const RECEIPT_PREFIX: &str = "receipt:";

/// Link and measured values available only after a signed receipt is stored.
#[derive(Debug, Clone)]
pub(crate) struct ReceiptLink {
    pub(crate) reference: String,
    pub(crate) provider_cost_micros: u64,
    pub(crate) tokens_used: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ObservedProviderEvidence {
    schema_version: u32,
    connector: String,
    provider: String,
    task_id: String,
    requested_model: String,
    selected_model: String,
    fresh_input_tokens: u64,
    cached_input_tokens: Option<u64>,
    output_tokens: u64,
    reasoning_tokens: Option<u64>,
    provider_cost_micros: u64,
    stdout_digest: String,
}

impl ObservedProviderEvidence {
    fn tokens_used(&self) -> TokenUsage {
        TokenUsage {
            input_tokens: self.fresh_input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cached_input_tokens.unwrap_or(0),
            cache_write_tokens: 0,
        }
    }
}

/// Record a receipt only for explicit provider JSON usage and cost evidence.
///
/// Failure to parse, sign, or persist evidence deliberately leaves the run
/// observed but unlinked; it never changes the connector's execution result.
pub(crate) fn record_provider_receipt(
    connector: &str,
    provider: &str,
    request: &TaskRequest,
    stdout: &str,
    duration_ms: u64,
) -> Option<ReceiptLink> {
    let evidence = observed_provider_evidence(connector, provider, request, stdout)?;
    persist_receipt(request, &evidence, duration_ms).ok()
}

/// Return the final human-visible agent response from a structured CLI stream.
/// Evidence parsing keeps the raw stream; task evaluation must score the answer,
/// not its JSON framing.
pub(crate) fn visible_output(raw_stdout: &str) -> String {
    let mut answer = None;
    for value in json_documents(raw_stdout) {
        for candidate in [
            value.get("result"),
            value.get("text"),
            value
                .get("message")
                .and_then(|message| message.get("content")),
            value.get("item").and_then(|item| item.get("text")),
            value.get("item").and_then(|item| item.get("content")),
        ] {
            if let Some(text) = candidate
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                answer = Some(text.to_owned());
            }
        }
    }
    answer.unwrap_or_else(|| raw_stdout.to_owned())
}

/// Verify the persisted receipt and its detached public key without network
/// access.  The provider evidence remains explicitly unverified.
pub(crate) fn verify_receipt_reference(reference: &str) -> anyhow::Result<bool> {
    let receipt_ref_id = receipt_id_from_reference(reference)?;
    let dir = receipt_directory()?;
    let receipt_path = dir.join(format!("{receipt_ref_id}.json"));
    let evidence_path = dir.join(format!("{receipt_ref_id}.evidence.json"));
    let key_path = dir.join(format!("{receipt_ref_id}.pub"));
    let receipt: ExecutionReceiptV1 = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    let evidence: ObservedProviderEvidence = serde_json::from_slice(&fs::read(&evidence_path)?)?;
    let public_key =
        hex_decode(fs::read_to_string(key_path)?.trim()).map_err(anyhow::Error::msg)?;

    if receipt.receipt_id.as_str() != receipt_ref_id {
        return Ok(false);
    }
    if receipt_id(&receipt)? != receipt_ref_id {
        return Ok(false);
    }
    receipt.validate().map_err(anyhow::Error::msg)?;
    let evidence_ref = receipt.evidence_refs.first();
    let expected_digest = evidence_digest(&evidence);
    if receipt.evidence_refs.len() != 1
        || !evidence_ref.is_some_and(|reference| {
            reference.kind == EvidenceKind::RuntimeLog
                && reference.uri == format!("receipt-evidence:{receipt_ref_id}")
                && reference.digest == expected_digest
                && reference.signature_status == SignatureStatus::Unverified
        })
    {
        return Ok(false);
    }

    verify_receipt_signature(&receipt, &public_key).map_err(anyhow::Error::msg)
}

fn observed_provider_evidence(
    connector: &str,
    provider: &str,
    request: &TaskRequest,
    stdout: &str,
) -> Option<ObservedProviderEvidence> {
    let stdout_digest = format!("blake3:{}", blake3::hash(stdout.as_bytes()).to_hex());
    json_documents(stdout).into_iter().find_map(|value| {
        let usage = normalize_connector_usage(connector, &value, request.model.as_deref());
        let fresh_input_tokens = usage.fresh_input_tokens?;
        let output_tokens = usage.output_tokens?;
        let provider_cost_micros = explicit_provider_cost_micros(&value)?;
        let selected_model = non_empty(&usage.model)
            .or_else(|| {
                request
                    .model
                    .as_deref()
                    .filter(|model| !model.trim().is_empty())
            })?
            .to_owned();
        let requested_model = request
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(&selected_model)
            .to_owned();

        Some(ObservedProviderEvidence {
            schema_version: 1,
            connector: connector.to_owned(),
            provider: provider.to_owned(),
            task_id: request.id.clone(),
            requested_model,
            selected_model,
            fresh_input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            provider_cost_micros,
            stdout_digest: stdout_digest.clone(),
        })
    })
}

fn json_documents(stdout: &str) -> Vec<Value> {
    let mut values = serde_json::from_str(stdout)
        .ok()
        .into_iter()
        .collect::<Vec<_>>();
    values.extend(
        stdout
            .lines()
            .filter_map(|line| serde_json::from_str(line.trim()).ok()),
    );
    values
}

fn normalize_connector_usage(
    connector: &str,
    value: &Value,
    requested_model: Option<&str>,
) -> NormalizedUsage {
    match connector {
        "claude-code" => normalize_anthropic(value, requested_model, 0),
        // Codex and Cursor CLI JSON both use OpenAI-style `usage` names when
        // they expose token evidence.  A cursor receipt still identifies
        // Cursor as its provider below; no upstream provider is inferred.
        _ => normalize_openai(value, requested_model, 0),
    }
}

fn explicit_provider_cost_micros(value: &Value) -> Option<u64> {
    let root = value.get("response").unwrap_or(value);
    [
        root.get("total_cost_usd"),
        root.get("cost_usd"),
        root.get("usage")
            .and_then(|usage| usage.get("total_cost_usd")),
        root.get("usage").and_then(|usage| usage.get("cost_usd")),
        root.get("usage").and_then(|usage| usage.get("cost")),
    ]
    .into_iter()
    .flatten()
    .find_map(decimal_value_to_micros)
}

fn persist_receipt(
    request: &TaskRequest,
    evidence: &ObservedProviderEvidence,
    duration_ms: u64,
) -> anyhow::Result<ReceiptLink> {
    let task_id = TaskId::try_from(request.id.clone()).map_err(anyhow::Error::msg)?;
    let plan_id = PlanId::try_from(format!("plan:{}", request.id)).map_err(anyhow::Error::msg)?;
    let billed_input_tokens = evidence
        .fresh_input_tokens
        .saturating_add(evidence.cached_input_tokens.unwrap_or(0));
    let evidence_digest = evidence_digest(evidence);
    let mut receipt = ExecutionReceiptV1 {
        schema_version: 1,
        receipt_id: ReceiptId::try_from("pending".to_owned()).map_err(anyhow::Error::msg)?,
        task_id,
        plan_id,
        context_balance: ContextBalanceV1 {
            original_tokens: billed_input_tokens,
            materialized_tokens: billed_input_tokens,
            delivered_tokens: billed_input_tokens,
            provider_billed_tokens: billed_input_tokens,
        },
        fresh_input_tokens: evidence.fresh_input_tokens,
        cached_input_tokens: evidence.cached_input_tokens.unwrap_or(0),
        output_tokens: evidence.output_tokens,
        reasoning_tokens: evidence.reasoning_tokens.unwrap_or(0),
        requested_model: evidence.requested_model.clone(),
        selected_model: evidence.selected_model.clone(),
        provider: evidence.provider.clone(),
        capability_id: None,
        capability_version: None,
        // CLI usage summaries do not prove internal provider-call or retry
        // counts, so zero denotes "not reported", never a zero-call claim.
        model_calls: 0,
        retries: 0,
        latency_ms: duration_ms,
        actual_cost_micros: evidence.provider_cost_micros,
        // A standalone connector execution makes no savings comparison.
        baseline_cost_micros: evidence.provider_cost_micros,
        avoided_cost_micros: 0,
        etpao_milli: 0,
        outcome_ref: None,
        knowledge_refs: Vec::new(),
        decision_refs: Vec::new(),
        evidence_refs: vec![EvidenceRefV1 {
            kind: EvidenceKind::RuntimeLog,
            uri: String::new(),
            digest: evidence_digest,
            signature_status: SignatureStatus::Unverified,
        }],
        signature: String::new(),
    };
    let receipt_id = receipt_id(&receipt)?;
    receipt.receipt_id = ReceiptId::try_from(receipt_id.clone()).map_err(anyhow::Error::msg)?;
    receipt.evidence_refs[0].uri = format!("receipt-evidence:{receipt_id}");

    let signing_key = get_or_create_keypair(current_agent_id()).map_err(anyhow::Error::msg)?;
    receipt.signature = sign_receipt(&receipt, &signing_key).map_err(anyhow::Error::msg)?;
    receipt.validate().map_err(anyhow::Error::msg)?;
    if !verify_receipt_signature(&receipt, &signing_key.verifying_key().to_bytes())
        .map_err(anyhow::Error::msg)?
    {
        anyhow::bail!("new execution receipt failed signature verification");
    }

    let dir = receipt_directory()?;
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join(format!("{receipt_id}.json")),
        canonical_serialize(&receipt),
    )?;
    fs::write(
        dir.join(format!("{receipt_id}.evidence.json")),
        canonical_serialize(evidence),
    )?;
    fs::write(
        dir.join(format!("{receipt_id}.pub")),
        hex_encode(&signing_key.verifying_key().to_bytes()),
    )?;

    Ok(ReceiptLink {
        reference: format!("{RECEIPT_PREFIX}{receipt_id}"),
        provider_cost_micros: evidence.provider_cost_micros,
        tokens_used: evidence.tokens_used(),
    })
}

fn receipt_directory() -> anyhow::Result<PathBuf> {
    Ok(crate::core::data_dir::lean_ctx_data_dir()
        .map_err(anyhow::Error::msg)?
        .join("execution-receipts"))
}

fn receipt_id(receipt: &ExecutionReceiptV1) -> anyhow::Result<String> {
    let mut unsigned = receipt.clone();
    unsigned.receipt_id = ReceiptId::try_from("pending".to_owned()).map_err(anyhow::Error::msg)?;
    unsigned.signature.clear();
    for evidence in &mut unsigned.evidence_refs {
        evidence.uri.clear();
    }
    Ok(format!(
        "receipt-{}",
        blake3::hash(&canonical_serialize(&unsigned)).to_hex()
    ))
}

fn receipt_id_from_reference(reference: &str) -> anyhow::Result<&str> {
    let receipt_id = reference
        .strip_prefix(RECEIPT_PREFIX)
        .filter(|receipt_id| !receipt_id.is_empty() && !receipt_id.contains(['/', '\\']))
        .ok_or_else(|| anyhow::anyhow!("invalid execution receipt reference"))?;
    ReceiptId::try_from(receipt_id).map_err(anyhow::Error::msg)?;
    Ok(receipt_id)
}

fn evidence_digest(evidence: &ObservedProviderEvidence) -> String {
    format!(
        "blake3:{}",
        blake3::hash(&canonical_serialize(evidence)).to_hex()
    )
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn request() -> TaskRequest {
        TaskRequest {
            id: "task-1".to_owned(),
            prompt: "test".to_owned(),
            working_dir: PathBuf::from("."),
            timeout_ms: 1_000,
            model: Some("claude-test".to_owned()),
            max_turns: None,
            profile_name: None,
            profile_hash: None,
        }
    }

    #[test]
    fn explicit_usage_and_cost_create_a_verifiable_receipt_link() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        let output = json!({
            "model": "claude-test",
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 25,
                "output_tokens": 40
            },
            "total_cost_usd": "0.000123"
        })
        .to_string();

        let link = record_provider_receipt("claude-code", "anthropic", &request(), &output, 42)
            .expect("explicit provider evidence should produce a receipt");

        assert_eq!(link.provider_cost_micros, 123);
        assert_eq!(link.tokens_used.input_tokens, 100);
        assert_eq!(link.tokens_used.cache_read_tokens, 25);
        assert!(verify_receipt_reference(&link.reference).expect("offline verifier should run"));
    }

    #[test]
    fn missing_cost_or_usage_never_creates_a_receipt() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        let missing_cost = json!({
            "model": "claude-test",
            "usage": {"input_tokens": 100, "output_tokens": 40}
        })
        .to_string();
        let missing_usage = json!({"model": "claude-test", "total_cost_usd": "0.01"}).to_string();

        assert!(
            record_provider_receipt("claude-code", "anthropic", &request(), &missing_cost, 1)
                .is_none()
        );
        assert!(
            record_provider_receipt("claude-code", "anthropic", &request(), &missing_usage, 1)
                .is_none()
        );
    }

    #[test]
    fn cursor_openai_style_json_is_supported_without_pricing_estimate() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        let output = json!({
            "model": "cursor-model",
            "usage": {
                "prompt_tokens": 20,
                "prompt_tokens_details": {"cached_tokens": 5},
                "completion_tokens": 8,
                "cost": "0.000004"
            }
        })
        .to_string();

        let link = record_provider_receipt("cursor", "cursor", &request(), &output, 7)
            .expect("explicit cursor evidence should produce a receipt");

        assert_eq!(link.provider_cost_micros, 4);
        assert_eq!(link.tokens_used.input_tokens, 15);
        assert!(verify_receipt_reference(&link.reference).expect("offline verifier should run"));
    }

    #[test]
    fn visible_output_uses_final_structured_answer() {
        let stream = [
            r#"{"item":{"text":"draft"}}"#,
            r#"{"result":"final answer"}"#,
        ]
        .join("\n");

        assert_eq!(visible_output(&stream), "final answer");
    }
}
