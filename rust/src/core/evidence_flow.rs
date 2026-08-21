//! End-to-end provider-run evidence assembly.
//!
//! This module is the boundary between a matched provider run and the
//! protocol receipts that make its savings claim independently verifiable.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use lean_ctx_protocol::{
    ContextBalanceV1, ExecutionReceiptV1, MeasurementMethod, PlanId, ReceiptId, SavingsReceiptV1,
    TaskId,
};
use serde_json::json;

use crate::cli::dispatch::analytics::provider_run::{ArmResult, ArmType};
use crate::cli::dispatch::verify::{VerifyCommand, execute_verify};
use crate::core::canonical::{AgentIdentity, canonical_serialize, sign_receipt};
use crate::core::evidence_bundle::generate_artifact_bundle;
use crate::core::quality_scorecard::QualityComparison;

const BASELINE_RECEIPT_FILE: &str = "execution-receipt-baseline.json";
const TREATMENT_RECEIPT_FILE: &str = "execution-receipt-treatment.json";
const SAVINGS_RECEIPT_FILE: &str = "savings-receipt.json";
const QUALITY_COMPARISON_FILE: &str = "quality-comparison.json";
const RUN_METADATA_FILE: &str = "run-metadata.json";
const BUNDLE_FILE: &str = "evidence-bundle.zip";

/// Inputs required to turn a completed matched run into signed evidence.
#[derive(Debug, Clone)]
pub struct EvidenceFlowConfig {
    pub run_id: String,
    pub run_dir: PathBuf,
    /// Raw 32-byte Ed25519 seed, or a UTF-8 file containing its hexadecimal
    /// representation. If absent, use LeanCTX's current agent identity.
    pub signing_identity_path: Option<PathBuf>,
}

/// Locations of the persisted artifacts and the result of the offline check.
#[derive(Debug, Clone)]
pub struct EvidenceFlowResult {
    pub bundle_path: PathBuf,
    pub execution_receipt_baseline: PathBuf,
    pub execution_receipt_treatment: PathBuf,
    pub savings_receipt: PathBuf,
    pub quality_comparison: PathBuf,
    pub verification_passed: bool,
}

/// Produce the complete signed evidence trail for a matched provider run.
pub fn build_evidence_from_run(
    config: &EvidenceFlowConfig,
    baseline_arm: &ArmResult,
    treatment_arm: &ArmResult,
    quality: &QualityComparison,
) -> Result<EvidenceFlowResult> {
    validate_inputs(config, baseline_arm, treatment_arm, quality)?;
    fs::create_dir_all(&config.run_dir)
        .with_context(|| format!("create evidence run directory {}", config.run_dir.display()))?;

    let signing_key = resolve_signing_identity(config)?;
    let plan_id = format!("provider-run-{}", config.run_id);
    let mut baseline_receipt = arm_to_receipt(baseline_arm, &config.run_id, &plan_id);
    let mut treatment_receipt = arm_to_receipt(treatment_arm, &config.run_id, &plan_id);

    treatment_receipt.baseline_cost_micros = baseline_receipt.actual_cost_micros;
    treatment_receipt.avoided_cost_micros = baseline_receipt
        .actual_cost_micros
        .saturating_sub(treatment_receipt.actual_cost_micros);
    baseline_receipt.signature = sign_receipt(&baseline_receipt, &signing_key)
        .map_err(|error| anyhow!("sign baseline execution receipt: {error}"))?;
    treatment_receipt.signature = sign_receipt(&treatment_receipt, &signing_key)
        .map_err(|error| anyhow!("sign treatment execution receipt: {error}"))?;
    baseline_receipt
        .validate()
        .map_err(|error| anyhow!("validate baseline execution receipt: {error}"))?;
    treatment_receipt
        .validate()
        .map_err(|error| anyhow!("validate treatment execution receipt: {error}"))?;

    let mut savings_receipt =
        SavingsReceiptV1::compute_from_arms(&baseline_receipt, &treatment_receipt);
    savings_receipt.quality_preserved = quality.treatment_scorecard.overall_score_milli
        >= quality.baseline_scorecard.overall_score_milli;
    savings_receipt.quality_baseline_score_milli = quality.baseline_scorecard.overall_score_milli;
    savings_receipt.quality_treatment_score_milli = quality.treatment_scorecard.overall_score_milli;
    savings_receipt.measurement_method = measurement_method(baseline_arm, treatment_arm);
    savings_receipt.context_strategy = "provider-run".to_string();
    savings_receipt.signature = sign_savings_receipt(&savings_receipt, &signing_key)?;
    savings_receipt
        .validate()
        .map_err(|error| anyhow!("validate savings receipt: {error}"))?;

    let baseline_path = config.run_dir.join(BASELINE_RECEIPT_FILE);
    let treatment_path = config.run_dir.join(TREATMENT_RECEIPT_FILE);
    let savings_path = config.run_dir.join(SAVINGS_RECEIPT_FILE);
    let quality_path = config.run_dir.join(QUALITY_COMPARISON_FILE);
    let metadata_path = config.run_dir.join(RUN_METADATA_FILE);
    let baseline_bytes = canonical_serialize(&baseline_receipt);
    let treatment_bytes = canonical_serialize(&treatment_receipt);
    let savings_bytes = canonical_serialize(&savings_receipt);
    let quality_bytes = canonical_serialize(quality);
    let metadata = json!({
        "run_id": config.run_id,
        "commit": baseline_arm.commit_sha,
        "model": baseline_arm.model,
        "provider": baseline_arm.provider,
        "timestamp": Utc::now().to_rfc3339(),
    });
    let metadata_bytes = canonical_serialize(&metadata);

    write_artifact(&baseline_path, &baseline_bytes)?;
    write_artifact(&treatment_path, &treatment_bytes)?;
    write_artifact(&savings_path, &savings_bytes)?;
    write_artifact(&quality_path, &quality_bytes)?;
    write_artifact(&metadata_path, &metadata_bytes)?;

    let bundle_path = config.run_dir.join(BUNDLE_FILE);
    generate_artifact_bundle(
        &bundle_path,
        vec![
            (BASELINE_RECEIPT_FILE.to_string(), baseline_bytes),
            (TREATMENT_RECEIPT_FILE.to_string(), treatment_bytes),
            (SAVINGS_RECEIPT_FILE.to_string(), savings_bytes),
            (QUALITY_COMPARISON_FILE.to_string(), quality_bytes),
            (RUN_METADATA_FILE.to_string(), metadata_bytes),
        ],
        &metadata,
        &signing_key,
    )
    .map_err(|error| anyhow!("generate evidence bundle: {error}"))?;

    let verification = execute_verify(&VerifyCommand {
        bundle_path: bundle_path.clone(),
        public_key: None,
        verbose: false,
        json_output: false,
    })
    .context("self-verify evidence bundle")?;
    if !verification.bundle_valid {
        bail!(
            "generated evidence bundle failed self-verification: {}",
            verification.errors.join("; ")
        );
    }

    Ok(EvidenceFlowResult {
        bundle_path,
        execution_receipt_baseline: baseline_path,
        execution_receipt_treatment: treatment_path,
        savings_receipt: savings_path,
        quality_comparison: quality_path,
        verification_passed: true,
    })
}

/// Convert the measurements from one provider-run arm into a protocol receipt.
#[must_use]
pub fn arm_to_receipt(arm: &ArmResult, run_id: &str, plan_id: &str) -> ExecutionReceiptV1 {
    let arm_name = match arm.arm_type {
        ArmType::Baseline => "baseline",
        ArmType::Treatment => "treatment",
    };
    let fresh_input_tokens = arm.input_tokens.saturating_sub(arm.cached_tokens);
    ExecutionReceiptV1 {
        schema_version: 1,
        receipt_id: ReceiptId::new(format!("execution-{run_id}-{arm_name}"))
            .expect("build_evidence_from_run validates receipt id length"),
        task_id: TaskId::new(run_id.to_string())
            .expect("build_evidence_from_run validates task id length"),
        plan_id: PlanId::new(plan_id.to_string())
            .expect("build_evidence_from_run validates plan id length"),
        context_balance: ContextBalanceV1 {
            original_tokens: arm.input_tokens,
            materialized_tokens: arm.input_tokens,
            delivered_tokens: arm.input_tokens,
            provider_billed_tokens: arm.input_tokens,
        },
        fresh_input_tokens,
        cached_input_tokens: arm.cached_tokens,
        output_tokens: arm.output_tokens,
        reasoning_tokens: 0,
        requested_model: arm.model.clone(),
        selected_model: arm.model.clone(),
        provider: arm.provider.clone(),
        capability_id: None,
        capability_version: None,
        model_calls: 1,
        retries: 0,
        latency_ms: arm.latency_ms,
        actual_cost_micros: arm.cost_micros,
        baseline_cost_micros: arm.cost_micros,
        avoided_cost_micros: 0,
        etpao_milli: 0,
        outcome_ref: None,
        knowledge_refs: Vec::new(),
        decision_refs: Vec::new(),
        evidence_refs: Vec::new(),
        signature: String::new(),
    }
}

fn validate_inputs(
    config: &EvidenceFlowConfig,
    baseline_arm: &ArmResult,
    treatment_arm: &ArmResult,
    quality: &QualityComparison,
) -> Result<()> {
    ensure!(
        !config.run_id.trim().is_empty(),
        "evidence run id must not be empty"
    );
    ensure!(
        config.run_id.len() <= 220,
        "evidence run id is too long for protocol receipt identifiers"
    );
    ensure!(
        baseline_arm.arm_type == ArmType::Baseline,
        "baseline receipt requires a baseline arm"
    );
    ensure!(
        treatment_arm.arm_type == ArmType::Treatment,
        "treatment receipt requires a treatment arm"
    );
    ensure!(
        !baseline_arm.provider.trim().is_empty() && !baseline_arm.model.trim().is_empty(),
        "baseline arm requires provider and model"
    );
    ensure!(
        !treatment_arm.provider.trim().is_empty() && !treatment_arm.model.trim().is_empty(),
        "treatment arm requires provider and model"
    );
    ensure!(
        baseline_arm.provider == treatment_arm.provider
            && baseline_arm.model == treatment_arm.model,
        "matched arms must use the same provider and model"
    );
    ensure!(
        baseline_arm.commit_sha == treatment_arm.commit_sha,
        "matched arms must evaluate the same commit"
    );
    ensure!(
        quality.baseline_scorecard.run_id == config.run_id
            && quality.treatment_scorecard.run_id == config.run_id,
        "quality scorecards must belong to the evidence run"
    );
    Ok(())
}

fn resolve_signing_identity(config: &EvidenceFlowConfig) -> Result<AgentIdentity> {
    let Some(path) = &config.signing_identity_path else {
        return crate::core::agent_identity::get_or_create_keypair(
            crate::core::agent_identity::current_agent_id(),
        )
        .map_err(|error| anyhow!("resolve LeanCTX signing identity: {error}"));
    };

    let bytes =
        fs::read(path).with_context(|| format!("read signing identity {}", path.display()))?;
    let seed = if bytes.len() == 32 {
        bytes
    } else {
        let hex_seed = std::str::from_utf8(&bytes)
            .context("signing identity must contain 32 raw bytes or hexadecimal UTF-8")?;
        hex::decode(hex_seed.trim()).context("decode hexadecimal signing identity")?
    };
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| anyhow!("signing identity must contain exactly 32 seed bytes"))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn sign_savings_receipt(receipt: &SavingsReceiptV1, identity: &AgentIdentity) -> Result<String> {
    let mut unsigned = serde_json::to_value(receipt).context("serialize savings receipt")?;
    unsigned
        .as_object_mut()
        .ok_or_else(|| anyhow!("savings receipt must serialize as a JSON object"))?
        .remove("signature");
    Ok(STANDARD.encode(identity.sign(&canonical_serialize(&unsigned)).to_bytes()))
}

fn measurement_method(baseline: &ArmResult, treatment: &ArmResult) -> MeasurementMethod {
    match (
        baseline.measurement_method.as_str(),
        treatment.measurement_method.as_str(),
    ) {
        ("provider_reported", "provider_reported") => MeasurementMethod::ProviderReported,
        ("unavailable", "unavailable") => MeasurementMethod::Unavailable,
        _ => MeasurementMethod::Estimated,
    }
}

fn write_artifact(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("write evidence artifact {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{Read, Write};

    use super::*;
    use crate::core::quality_scorecard::{
        DimensionScore, QualityDimension, QualityScorecard, ScoreConfidence,
    };

    fn arm(arm_type: ArmType) -> ArmResult {
        let baseline = arm_type == ArmType::Baseline;
        ArmResult {
            arm_type,
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            commit_sha: "abc123".to_string(),
            input_tokens: if baseline { 1_000 } else { 600 },
            cached_tokens: if baseline { 0 } else { 100 },
            output_tokens: 200,
            cost_micros: if baseline { 10_000 } else { 6_000 },
            latency_ms: 50,
            output_content: "synthetic provider output".to_string(),
            proxy_observed: !baseline,
            measurement_method: "provider_reported".to_string(),
        }
    }

    fn quality() -> QualityComparison {
        let scorecard = |arm_type: &str| QualityScorecard {
            scorecard_id: format!("scorecard-{arm_type}"),
            run_id: "run-1".to_string(),
            arm_type: arm_type.to_string(),
            dimensions: vec![DimensionScore {
                dimension: QualityDimension::Correctness,
                score_milli: 900,
                confidence: ScoreConfidence::Automated,
                notes: None,
            }],
            overall_score_milli: 900,
            reviewer: "test".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };
        let baseline = scorecard("baseline");
        let treatment = scorecard("treatment");
        QualityComparison::compare(&baseline, &treatment, 50)
    }

    fn build_flow() -> (tempfile::TempDir, EvidenceFlowResult) {
        let temp = tempfile::tempdir().expect("tempdir");
        let key_path = temp.path().join("signing.key");
        fs::write(&key_path, [7_u8; 32]).expect("write test signing key");
        let result = build_evidence_from_run(
            &EvidenceFlowConfig {
                run_id: "run-1".to_string(),
                run_dir: temp.path().join("run"),
                signing_identity_path: Some(key_path),
            },
            &arm(ArmType::Baseline),
            &arm(ArmType::Treatment),
            &quality(),
        )
        .expect("build evidence flow");
        (temp, result)
    }

    #[test]
    fn synthetic_arms_produce_a_self_verified_bundle() {
        let (_temp, result) = build_flow();

        assert!(result.verification_passed);
        assert!(result.bundle_path.is_file());
        assert!(result.execution_receipt_baseline.is_file());
        assert!(result.execution_receipt_treatment.is_file());
        assert!(result.savings_receipt.is_file());
        assert!(result.quality_comparison.is_file());
        assert!(
            execute_verify(&VerifyCommand {
                bundle_path: result.bundle_path,
                public_key: None,
                verbose: false,
                json_output: false,
            })
            .expect("verify bundle")
            .bundle_valid
        );
    }

    #[test]
    fn tampered_receipt_fails_bundle_verification() {
        let (_temp, result) = build_flow();
        tamper_zip_entry(&result.bundle_path, BASELINE_RECEIPT_FILE);

        let verification = execute_verify(&VerifyCommand {
            bundle_path: result.bundle_path,
            public_key: None,
            verbose: false,
            json_output: false,
        })
        .expect("verify tampered bundle");
        assert!(!verification.bundle_valid, "{verification:?}");
        assert!(
            verification
                .errors
                .iter()
                .any(|error| error.contains(BASELINE_RECEIPT_FILE))
        );
    }

    fn tamper_zip_entry(path: &Path, target: &str) {
        let file = File::open(path).expect("open bundle");
        let mut archive = zip::ZipArchive::new(file).expect("read bundle");
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("read entry");
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read entry bytes");
            if name == target {
                bytes[0] ^= 1;
            }
            entries.push((name, bytes));
        }

        let mut output = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut output));
            let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .last_modified_time(zip::DateTime::default());
            for (name, bytes) in entries {
                zip.start_file(name, options).expect("write ZIP entry");
                zip.write_all(&bytes).expect("write ZIP bytes");
            }
            zip.finish().expect("finish ZIP");
        }
        fs::write(path, output).expect("replace bundle");
    }
}
