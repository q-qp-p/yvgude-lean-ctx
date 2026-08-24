//! Independent mutation tests for customer-proof evidence-bundle-v2.
//!
//! The engine is not linked. This builds a proof from the published V2 vector,
//! materializes its sidecar artifacts, and verifies only through the standalone
//! implementation plus an out-of-band trust store.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "../src/receipt.rs"]
mod receipt;
#[path = "../src/v2.rs"]
mod v2;
#[path = "../src/verify.rs"]
#[allow(dead_code)]
mod verify;

static FIXTURE_INDEX: AtomicUsize = AtomicUsize::new(0);

fn canonical(value: &Value) -> Vec<u8> {
    fn sort(value: Value) -> Value {
        match value {
            Value::Array(values) => Value::Array(values.into_iter().map(sort).collect()),
            Value::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, sort(value)))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            scalar => scalar,
        }
    }
    serde_json::to_vec(&sort(value.clone())).unwrap()
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unsigned(value: &Value) -> Value {
    let mut unsigned = value.clone();
    let root = unsigned.as_object_mut().unwrap();
    root.remove("bundle_id");
    root.remove("bundle_digest");
    let signing = root.get_mut("signing").unwrap().as_object_mut().unwrap();
    signing.remove("signed_digest");
    signing.remove("signature");
    unsigned
}

fn sign(value: &mut Value, signing_key: &SigningKey) {
    let unsigned = unsigned(value);
    let bundle_digest = digest(&canonical(&unsigned));
    let root = value.as_object_mut().unwrap();
    root.insert("bundle_digest".into(), Value::String(bundle_digest.clone()));
    root.insert(
        "bundle_id".into(),
        Value::String(format!("id:{bundle_digest}")),
    );
    let signing = root.get_mut("signing").unwrap().as_object_mut().unwrap();
    signing.insert("signed_digest".into(), Value::String(bundle_digest));
    signing.insert(
        "signature".into(),
        Value::String(STANDARD.encode(signing_key.sign(&canonical(&unsigned)).to_bytes())),
    );
}

struct ProofFixture {
    root: PathBuf,
    bundle: Vec<u8>,
    trust_store: Vec<u8>,
}

impl Drop for ProofFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn rewrite_refs(value: &mut Value, replacements: &BTreeMap<String, String>) {
    match value {
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| rewrite_refs(value, replacements)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| rewrite_refs(value, replacements)),
        Value::String(value) => {
            if let Some(replacement) = replacements.get(value) {
                *value = replacement.clone();
            }
        }
        _ => {}
    }
}

fn write_artifact(root: &Path, path: &str, bytes: &[u8]) {
    let destination = root.join(path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, bytes).unwrap();
}

struct ReceiptRefs<'a> {
    task_ref: &'a str,
    plan_ref: &'a str,
    invocation_ref: &'a str,
    identity_ref: &'a str,
    policy_ref: &'a str,
    evidence_digest: &'a str,
}

fn signed_receipt(role: &str, key: &SigningKey, key_id: &str, refs: ReceiptRefs<'_>) -> Vec<u8> {
    let ReceiptRefs {
        task_ref,
        plan_ref,
        invocation_ref,
        identity_ref,
        policy_ref,
        evidence_digest,
    } = refs;
    let mut receipt = serde_json::json!({
        "schema_version": 1,
        "receipt_id": format!("sha256:{}", "0".repeat(64)),
        "lineage": {
            "task_id": format!("task-{role}"),
            "task_ref": task_ref,
            "plan_id": format!("plan-{role}"),
            "plan_ref": plan_ref,
            "invocation_id": format!("invocation-{role}"),
            "invocation_ref": invocation_ref,
            "identity_ref": identity_ref,
            "policy_refs": [policy_ref],
            "capabilities": [{
                "capability_id": "capability://leanctx/context",
                "capability_version": "1.0.0",
                "invocation_ref": invocation_ref
            }]
        },
        "chain": {"chain_id": format!("chain-{role}"), "sequence_number": 1},
        "status": "succeeded",
        "values": [{
            "name": "input_tokens",
            "unit": "token",
            "classification": "measured",
            "value": 42,
            "evidence_digests": [evidence_digest]
        }],
        "outcome": {"state": "unknown"},
        "evidence_refs": [{
            "kind": "measurement",
            "uri": format!("artifact://{role}/measurement"),
            "digest": evidence_digest,
            "media_type": "application/json",
            "signature_status": "Verified"
        }],
        "issued_at": "2026-08-22T09:00:00Z",
        "signer": {
            "algorithm": "ed25519",
            "key_id": key_id,
            "key_admission": "external_trust_store"
        },
        "signature": ""
    });
    let mut identity = receipt.clone();
    identity.as_object_mut().unwrap().remove("receipt_id");
    identity.as_object_mut().unwrap().remove("signature");
    receipt["receipt_id"] = Value::String(digest(&canonical(&identity)));
    let mut signing = receipt.clone();
    signing.as_object_mut().unwrap().remove("signature");
    receipt["signature"] =
        Value::String(STANDARD.encode(key.sign(&canonical(&signing)).to_bytes()));
    canonical(&receipt)
}

fn resign_receipt(receipt: &mut Value, key: &SigningKey) {
    receipt["receipt_id"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    receipt["signature"] = Value::String(String::new());
    let mut identity = receipt.clone();
    identity.as_object_mut().unwrap().remove("receipt_id");
    identity.as_object_mut().unwrap().remove("signature");
    receipt["receipt_id"] = Value::String(digest(&canonical(&identity)));
    let mut signing = receipt.clone();
    signing.as_object_mut().unwrap().remove("signature");
    receipt["signature"] =
        Value::String(STANDARD.encode(key.sign(&canonical(&signing)).to_bytes()));
}

fn mutate_control_receipt(
    proof: &mut ProofFixture,
    mutate: impl FnOnce(&mut Value),
    mutate_trust: impl FnOnce(&mut Value),
) {
    let key = SigningKey::from_bytes(&[17; 32]);
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    let items = bundle["inventory"]["items"].as_array_mut().unwrap();
    let item = items
        .iter_mut()
        .find(|item| item["path"] == "arms/control.json")
        .unwrap();
    let old_ref = item["ref"].as_str().unwrap().to_owned();
    let mut receipt: Value =
        serde_json::from_slice(&fs::read(proof.root.join("arms/control.json")).unwrap()).unwrap();
    mutate(&mut receipt);
    resign_receipt(&mut receipt, &key);
    let bytes = canonical(&receipt);
    write_artifact(&proof.root, "arms/control.json", &bytes);
    let artifact_digest = digest(&bytes);
    let reference = format!("id:{artifact_digest}");
    item["digest"] = Value::String(artifact_digest);
    item["ref"] = Value::String(reference.clone());
    item["size_bytes"] = Value::from(bytes.len());
    rewrite_refs(&mut bundle, &BTreeMap::from([(old_ref, reference)]));
    for item in bundle["inventory"]["items"].as_array_mut().unwrap() {
        if item["kind"] == "receipt_predecessor" {
            item["ref"] = Value::String(format!("id:{}", item["digest"].as_str().unwrap()));
        }
    }
    let total_bytes: u64 = bundle["inventory"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["availability"] == "present")
        .map(|item| item["size_bytes"].as_u64().unwrap())
        .sum();
    bundle["inventory"]["total_bytes"] = Value::from(total_bytes);

    let mut trust: Value = serde_json::from_slice(&proof.trust_store).unwrap();
    let head = trust["receipt_chain_heads"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|head| head["chain_id"] == receipt["chain"]["chain_id"])
        .unwrap();
    head["sequence_number"] = receipt["chain"]["sequence_number"].clone();
    head["receipt_id"] = receipt["receipt_id"].clone();
    mutate_trust(&mut trust);
    sign(&mut bundle, &key);
    proof.bundle = canonical(&bundle);
    proof.trust_store = canonical(&trust);
}

fn add_control_predecessor(proof: &mut ProofFixture) -> (String, String) {
    let bytes = fs::read(proof.root.join("arms/control.json")).unwrap();
    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
    let receipt_id = receipt["receipt_id"].as_str().unwrap().to_owned();
    let signature = STANDARD
        .decode(receipt["signature"].as_str().unwrap())
        .unwrap();
    let signature_digest = digest(&signature);
    write_artifact(&proof.root, "arms/control-predecessor.json", &bytes);
    let artifact_digest = digest(&bytes);
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    bundle["inventory"]["items"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "availability": "present",
            "digest": artifact_digest.clone(),
            "kind": "receipt_predecessor",
            "path": "arms/control-predecessor.json",
            "redaction_class": "metadata_only",
            "ref": format!("id:{artifact_digest}"),
            "size_bytes": bytes.len()
        }));
    bundle["inventory"]["item_count"] =
        Value::from(bundle["inventory"]["items"].as_array().unwrap().len());
    bundle["inventory"]["total_bytes"] = Value::from(
        bundle["inventory"]["total_bytes"].as_u64().unwrap() + u64::try_from(bytes.len()).unwrap(),
    );
    proof.bundle = canonical(&bundle);
    (receipt_id, signature_digest)
}

fn add_inventory_artifact(
    proof: &mut ProofFixture,
    kind: &str,
    path: &str,
    bytes: &[u8],
) -> String {
    write_artifact(&proof.root, path, bytes);
    let artifact_digest = digest(bytes);
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    bundle["inventory"]["items"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "availability": "present",
            "digest": artifact_digest.clone(),
            "kind": kind,
            "path": path,
            "redaction_class": "metadata_only",
            "ref": format!("id:{artifact_digest}"),
            "size_bytes": bytes.len()
        }));
    bundle["inventory"]["item_count"] =
        Value::from(bundle["inventory"]["items"].as_array().unwrap().len());
    bundle["inventory"]["total_bytes"] = Value::from(
        bundle["inventory"]["total_bytes"].as_u64().unwrap() + u64::try_from(bytes.len()).unwrap(),
    );
    proof.bundle = canonical(&bundle);
    artifact_digest
}

fn replace_json_artifact(
    proof: &mut ProofFixture,
    path: &str,
    mutate: impl FnOnce(&mut Value),
) -> (String, String) {
    let mut document: Value =
        serde_json::from_slice(&fs::read(proof.root.join(path)).unwrap()).unwrap();
    mutate(&mut document);
    let bytes = canonical(&document);
    write_artifact(&proof.root, path, &bytes);

    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    let item = bundle["inventory"]["items"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["path"] == path)
        .unwrap();
    let old_digest = item["digest"].as_str().unwrap().to_owned();
    let old_ref = item["ref"].as_str().unwrap().to_owned();
    let old_size = item["size_bytes"].as_u64().unwrap();
    let new_digest = digest(&bytes);
    let new_ref = format!("id:{new_digest}");
    item["digest"] = Value::String(new_digest.clone());
    item["ref"] = Value::String(new_ref.clone());
    item["size_bytes"] = Value::from(bytes.len());
    bundle["inventory"]["total_bytes"] = Value::from(
        bundle["inventory"]["total_bytes"].as_u64().unwrap() - old_size
            + u64::try_from(bytes.len()).unwrap(),
    );
    rewrite_refs(
        &mut bundle,
        &BTreeMap::from([(old_ref, new_ref), (old_digest.clone(), new_digest.clone())]),
    );
    proof.bundle = canonical(&bundle);
    (old_digest, new_digest)
}

fn fixture() -> ProofFixture {
    let index = FIXTURE_INDEX.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("leanctx-v2-{}-{index}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut bundle: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/evidence-bundle-v2/valid.json"
    ))
    .unwrap();
    let key = SigningKey::from_bytes(&[17; 32]);
    let key_id = format!("id:{}", digest(key.verifying_key().as_bytes()));
    for (kind, path) in [
        ("run_metadata", "lineage/identity.json"),
        ("claim_basis", "lineage/policy.json"),
        ("task_envelope", "lineage/control-task.json"),
        ("execution_plan", "lineage/control-plan.json"),
        ("engine_invocation", "lineage/control-invocation.json"),
        ("task_envelope", "lineage/treatment-task.json"),
        ("execution_plan", "lineage/treatment-plan.json"),
        ("engine_invocation", "lineage/treatment-invocation.json"),
    ] {
        let placeholder = digest(path.as_bytes());
        bundle["inventory"]["items"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "availability": "present",
                "digest": placeholder.clone(),
                "kind": kind,
                "path": path,
                "redaction_class": "metadata_only",
                "ref": format!("id:{placeholder}"),
                "size_bytes": 0
            }));
    }
    bundle["inventory"]["item_count"] =
        Value::from(bundle["inventory"]["items"].as_array().unwrap().len());
    let mut replacements = BTreeMap::new();
    let mut total_bytes = 0_u64;
    let mut sidecars = BTreeMap::new();
    for item in bundle["inventory"]["items"].as_array_mut().unwrap() {
        let kind = item["kind"].as_str().unwrap().to_owned();
        if kind == "arm_receipt" {
            continue;
        }
        let previous = item["ref"].as_str().unwrap().to_owned();
        let path = item["path"].as_str().unwrap().to_owned();
        let role = if path.contains("control") {
            Some("control")
        } else if path.contains("treatment") {
            Some("treatment")
        } else {
            None
        };
        let bytes = match (kind.as_str(), role) {
            ("run_metadata", None) => canonical(&Value::String("agent-fixture".into())),
            ("claim_basis", None) => canonical(&serde_json::json!({
                "policy_ref": "policy:fixture",
                "decision": "admitted"
            })),
            ("task_envelope", Some(role)) => canonical(&serde_json::json!({
                "schema_version": 1,
                "task_id": format!("task-{role}"),
                "trace_id": format!("trace-{role}"),
                "project_id": "project-fixture",
                "session_id": "session-fixture",
                "agent_id": "agent-fixture",
                "complexity": "medium",
                "created_at": "2026-08-22T08:59:00Z"
            })),
            ("execution_plan", Some(role)) => canonical(&serde_json::json!({
                "schema_version": 1,
                "plan_id": format!("plan-{role}"),
                "task_id": format!("task-{role}"),
                "context_budget_tokens": 1000,
                "context_strategy": "balanced",
                "knowledge_refs": [],
                "capability_ids": ["capability://leanctx/context"],
                "model": "local-engine",
                "provider": "leanctx",
                "reasoning_allocation_milli": 0,
                "max_retries": 0,
                "fallback_refs": [],
                "stop_condition": "on_completion",
                "expected_cost_micros": 0,
                "expected_quality_milli": 900,
                "expected_latency_ms": 100
            })),
            ("engine_invocation", Some(role)) => canonical(&serde_json::json!({
                "schema_version": 1,
                "invocation_id": format!("invocation-{role}"),
                "engine": {"engine_id": "leanctx", "engine_version": "1.0.0"},
                "operation": {
                    "capability_id": "capability://leanctx/context",
                    "capability_version": "1.0.0"
                },
                "input_ref": format!("id:{}", sidecars["replay_input-shared"]),
                "input_digest": sidecars["replay_input-shared"],
                "source_refs": [format!("id:{}", sidecars["replay_input-shared"])],
                "policy_admission": {
                    "policy_ref": "policy:fixture",
                    "decision": "admitted"
                }
            })),
            _ => format!("proof-artifact:{path}").into_bytes(),
        };
        write_artifact(&root, &path, &bytes);
        let artifact_digest = digest(&bytes);
        let reference = format!("id:{artifact_digest}");
        item["digest"] = Value::String(artifact_digest.clone());
        item["ref"] = Value::String(reference.clone());
        item["size_bytes"] = Value::from(bytes.len());
        total_bytes += u64::try_from(bytes.len()).unwrap();
        replacements.insert(previous, reference);
        sidecars.insert(
            format!("{kind}-{}", role.unwrap_or("shared")),
            artifact_digest,
        );
    }
    for item in bundle["inventory"]["items"].as_array_mut().unwrap() {
        if item["kind"].as_str() != Some("arm_receipt") {
            continue;
        }
        let previous = item["ref"].as_str().unwrap().to_owned();
        let path = item["path"].as_str().unwrap().to_owned();
        let role = if path.contains("control") {
            "control"
        } else {
            "treatment"
        };
        let bytes = signed_receipt(
            role,
            &key,
            &key_id,
            ReceiptRefs {
                task_ref: &sidecars[&format!("task_envelope-{role}")],
                plan_ref: &sidecars[&format!("execution_plan-{role}")],
                invocation_ref: &sidecars[&format!("engine_invocation-{role}")],
                identity_ref: &sidecars["run_metadata-shared"],
                policy_ref: &sidecars["claim_basis-shared"],
                evidence_digest: &sidecars["quality_measurement-shared"],
            },
        );
        write_artifact(&root, &path, &bytes);
        let artifact_digest = digest(&bytes);
        let reference = format!("id:{artifact_digest}");
        item["digest"] = Value::String(artifact_digest);
        item["ref"] = Value::String(reference.clone());
        item["size_bytes"] = Value::from(bytes.len());
        total_bytes += u64::try_from(bytes.len()).unwrap();
        replacements.insert(previous, reference);
    }
    bundle["inventory"]["total_bytes"] = Value::from(total_bytes);
    rewrite_refs(&mut bundle, &replacements);

    bundle["signing"]["key_id"] = Value::String(key_id.clone());
    bundle["signing"]["trusted_signer_ref"] = Value::String(format!("signer:{key_id}"));
    bundle["signing"]["trust_basis"] = Value::String("out_of_band".into());
    sign(&mut bundle, &key);
    let receipt_chain_heads: Vec<Value> = ["arms/control.json", "arms/treatment.json"]
        .iter()
        .map(|path| {
            let receipt: Value =
                serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap();
            serde_json::json!({
                "chain_id": receipt["chain"]["chain_id"],
                "sequence_number": receipt["chain"]["sequence_number"],
                "receipt_id": receipt["receipt_id"]
            })
        })
        .collect();
    let trust_store = serde_json::json!({
        "schema_version": "leanctx.customer-proof-trust-store/v1",
        "trust_revision": 1,
        "evaluated_at": "2026-08-24T00:00:00Z",
        "trusted_signers": [{
            "trusted_signer_ref": format!("signer:{key_id}"),
            "key_id": key_id.clone(),
            "public_key": hex(key.verifying_key().as_bytes()),
            "allowed_trust_bases": ["out_of_band"],
            "receipt_key_ids": [key_id],
            "revision": 1,
            "admitted_at": "2026-01-01T00:00:00Z",
            "expires_at": "2027-01-01T00:00:00Z",
            "revoked_at": null
        }],
        "receipt_chain_heads": receipt_chain_heads
    });
    ProofFixture {
        root,
        bundle: canonical(&bundle),
        trust_store: canonical(&trust_store),
    }
}

fn assert_structure_rejected(mutate: impl FnOnce(&mut Value)) {
    let proof = fixture();
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    mutate(&mut bundle);
    sign(&mut bundle, &SigningKey::from_bytes(&[17; 32]));
    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "v2 structure");
}

#[test]
fn trusted_canonical_v2_proof_with_artifacts_verifies() {
    let proof = fixture();
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);
    assert!(report.proof_eligible);
}

#[test]
fn v2_requires_external_trust_store() {
    let proof = fixture();
    let report = v2::verify_v2_document(&proof.bundle, None, Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "external signer trust");
}

#[test]
fn v2_rejects_tampered_artifact_before_claim_promotion() {
    let proof = fixture();
    fs::write(proof.root.join("arms/control.json"), b"tampered").unwrap();
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "artifact inventory");
}

#[test]
fn v2_rejects_supported_cost_claim_without_lower_observed_cost() {
    let proof = fixture();
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    bundle["matched_arms"]["treatment"]["measurements"]["cost"]["amount_micros"] =
        Value::from(1_000_000_u64);
    let key = SigningKey::from_bytes(&[17; 32]);
    sign(&mut bundle, &key);
    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "semantic joins");
}

#[test]
fn v2_rejects_partial_broad_supported_claim() {
    let proof = fixture();
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    bundle["status"] = Value::String("partial".into());
    bundle["claims"][0]["scope"] = Value::String("customer_workload".into());
    let key = SigningKey::from_bytes(&[17; 32]);
    sign(&mut bundle, &key);
    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "semantic joins");
}

#[test]
fn v2_rejects_duplicate_json_keys_before_interpreting_document() {
    let proof = fixture();
    let raw = br#"{"bundle_kind":"customer-proof","bundle_kind":"customer-proof"}"#;
    let report = v2::verify_v2_document(raw, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "canonical JSON");
}

#[test]
fn v2_rejects_impossible_rfc3339_created_at_before_trust_evaluation() {
    let proof = fixture();
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    bundle["created_at"] = Value::String("2026-02-31T12:00:00Z".into());

    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );

    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "v2 structure");
}

#[test]
fn v2_rejects_year_zero_timestamp() {
    assert_structure_rejected(|bundle| {
        bundle["created_at"] = Value::String("0000-01-01T00:00:00Z".into());
    });
}

#[test]
fn v2_rejects_malformed_subject_and_identity_names() {
    assert_structure_rejected(|bundle| {
        bundle["subject"]["customer_ref"] = Value::String("customer:A".into());
    });
    assert_structure_rejected(|bundle| {
        bundle["matched_arms"]["shared_identity"]["provider"] = Value::String("A".into());
    });
    assert_structure_rejected(|bundle| {
        bundle["matched_arms"]["shared_identity"]["endpoint_ref"] = Value::Null;
    });
}

#[test]
fn v2_enforces_text_and_limitation_bounds() {
    assert_structure_rejected(|bundle| {
        bundle["replay"]["notes"] = Value::String("x".repeat(1025));
    });
    assert_structure_rejected(|bundle| {
        bundle["redaction"]["notes"] = Value::String("é".repeat(513));
    });
    assert_structure_rejected(|bundle| {
        bundle["claims"][0]["statement"] = Value::String("x".repeat(513));
    });
    assert_structure_rejected(|bundle| {
        bundle["limitations"]["known_limitations"] = serde_json::json!([7]);
    });
    assert_structure_rejected(|bundle| {
        bundle["limitations"]["unproven"] = serde_json::json!(["invented"]);
    });
}

#[test]
fn v2_rejects_receipt_identity_and_policy_payload_mismatch() {
    let mut proof = fixture();
    let other_identity = canonical(&Value::String("agent-other".into()));
    let identity_digest = add_inventory_artifact(
        &mut proof,
        "run_metadata",
        "lineage/other-identity.json",
        &other_identity,
    );
    mutate_control_receipt(
        &mut proof,
        |receipt| receipt["lineage"]["identity_ref"] = Value::String(identity_digest),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");

    let mut proof = fixture();
    let other_policy = canonical(&serde_json::json!({
        "policy_ref": "policy:other",
        "decision": "admitted"
    }));
    let policy_digest = add_inventory_artifact(
        &mut proof,
        "claim_basis",
        "lineage/other-policy.json",
        &other_policy,
    );
    mutate_control_receipt(
        &mut proof,
        |receipt| receipt["lineage"]["policy_refs"] = serde_json::json!([policy_digest]),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_invocation_source_ref_not_in_inventory() {
    let mut proof = fixture();
    let (old_invocation, new_invocation) =
        replace_json_artifact(&mut proof, "lineage/control-invocation.json", |document| {
            let input_ref = document["input_ref"].clone();
            document["source_refs"] =
                serde_json::json!([input_ref, format!("id:sha256:{}", "0".repeat(64))]);
        });
    mutate_control_receipt(
        &mut proof,
        |receipt| rewrite_refs(receipt, &BTreeMap::from([(old_invocation, new_invocation)])),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_unknown_inventory_kind_before_trust_evaluation() {
    let proof = fixture();
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    bundle["inventory"]["items"][0]["kind"] = Value::String("opaque_receipt".into());

    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );

    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "v2 structure");
}

#[test]
fn v2_rejects_revoked_external_signer_snapshot() {
    let proof = fixture();
    let mut trust: Value = serde_json::from_slice(&proof.trust_store).unwrap();
    trust["trusted_signers"][0]["revoked_at"] = Value::String("2026-08-23T00:00:00Z".into());
    let report = v2::verify_v2_document(&proof.bundle, Some(&canonical(&trust)), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "external signer trust");
}

#[test]
fn v2_rejects_receipt_rollback_against_external_chain_head() {
    let proof = fixture();
    let mut trust: Value = serde_json::from_slice(&proof.trust_store).unwrap();
    trust["receipt_chain_heads"][0]["sequence_number"] = Value::from(2_u64);
    let report = v2::verify_v2_document(&proof.bundle, Some(&canonical(&trust)), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_resigned_bundle_with_tampered_receipt_signature() {
    let proof = fixture();
    let receipt_path = proof.root.join("arms/control.json");
    let old_bytes = fs::read(&receipt_path).unwrap();
    let old_digest = digest(&old_bytes);
    let old_ref = format!("id:{old_digest}");
    let mut receipt: Value = serde_json::from_slice(&old_bytes).unwrap();
    receipt["values"][0]["value"] = Value::from(43_u64);
    let new_bytes = canonical(&receipt);
    write_artifact(&proof.root, "arms/control.json", &new_bytes);

    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    let item = bundle["inventory"]["items"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["path"].as_str() == Some("arms/control.json"))
        .unwrap();
    let new_digest = digest(&new_bytes);
    let new_ref = format!("id:{new_digest}");
    let previous_size = item["size_bytes"].as_u64().unwrap();
    item["digest"] = Value::String(new_digest);
    item["ref"] = Value::String(new_ref.clone());
    item["size_bytes"] = Value::from(new_bytes.len());
    let total = bundle["inventory"]["total_bytes"].as_u64().unwrap() - previous_size
        + u64::try_from(new_bytes.len()).unwrap();
    bundle["inventory"]["total_bytes"] = Value::from(total);
    rewrite_refs(&mut bundle, &BTreeMap::from([(old_ref, new_ref)]));
    sign(&mut bundle, &SigningKey::from_bytes(&[17; 32]));

    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_receipt_lineage_payload_mismatch() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| receipt["lineage"]["task_id"] = Value::String("task-other".into()),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

fn add_control_outcome(proof: &mut ProofFixture, task_id: &str) -> (String, String) {
    let bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    let measurement_digest = bundle["inventory"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "quality_measurement")
        .unwrap()["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let outcome = canonical(&serde_json::json!({
        "schema_version": 1,
        "outcome_id": "outcome-control",
        "task_id": task_id,
        "accepted": "accepted",
        "quality_score_milli": 950,
        "signals": {
            "build": null,
            "tests": "passed",
            "lint": null,
            "typecheck": null,
            "completion": "passed",
            "pr": null,
            "correction": null,
            "rollback": null,
            "retry": null
        },
        "evidence_refs": [{
            "kind": "QualityMeasurement",
            "uri": "artifact://control/measurement",
            "digest": measurement_digest,
            "signature_status": "Verified"
        }],
        "observed_at": "2026-08-22T09:01:00Z"
    }));
    let outcome_digest =
        add_inventory_artifact(proof, "accepted_outcome", "outcomes/control.json", &outcome);
    (outcome_digest, measurement_digest)
}

#[test]
fn v2_verifies_exact_accepted_outcome_payload_join() {
    let mut proof = fixture();
    let (outcome_digest, acceptance_digest) = add_control_outcome(&mut proof, "task-control");
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["outcome"] = serde_json::json!({
                "state": "accepted",
                "outcome_id": "outcome-control",
                "outcome_ref": outcome_digest,
                "acceptance_evidence_digest": acceptance_digest
            });
            receipt["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "kind": "outcome",
                    "uri": "artifact://control/outcome",
                    "digest": outcome_digest,
                    "media_type": "application/json",
                    "signature_status": "Verified"
                }));
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);
}

#[test]
fn v2_rejects_accepted_outcome_task_mismatch() {
    let mut proof = fixture();
    let (outcome_digest, acceptance_digest) = add_control_outcome(&mut proof, "task-other");
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["outcome"] = serde_json::json!({
                "state": "accepted",
                "outcome_id": "outcome-control",
                "outcome_ref": outcome_digest,
                "acceptance_evidence_digest": acceptance_digest
            });
            receipt["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "kind": "outcome",
                    "uri": "artifact://control/outcome",
                    "digest": outcome_digest,
                    "media_type": "application/json",
                    "signature_status": "Verified"
                }));
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_accepted_outcome_payload_digest_not_listed_by_receipt() {
    let mut proof = fixture();
    let (outcome_digest, acceptance_digest) = add_control_outcome(&mut proof, "task-control");
    let (old_outcome, new_outcome) =
        replace_json_artifact(&mut proof, "outcomes/control.json", |document| {
            document["evidence_refs"][0]["digest"] =
                Value::String(format!("sha256:{}", "0".repeat(64)));
        });
    assert_eq!(old_outcome, outcome_digest);
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["outcome"] = serde_json::json!({
                "state": "accepted",
                "outcome_id": "outcome-control",
                "outcome_ref": new_outcome.clone(),
                "acceptance_evidence_digest": acceptance_digest
            });
            receipt["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "kind": "outcome",
                    "uri": "artifact://control/outcome",
                    "digest": new_outcome,
                    "media_type": "application/json",
                    "signature_status": "Verified"
                }));
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_noncanonical_ed25519_signature_pad_bits() {
    let proof = fixture();
    let mut bundle: Value = serde_json::from_slice(&proof.bundle).unwrap();
    let signature = bundle["signing"]["signature"].as_str().unwrap();
    let mut chars: Vec<char> = signature.chars().collect();
    let last_data = chars.len() - 3;
    chars[last_data] = match chars[last_data] {
        'A' => 'B',
        'Q' => 'R',
        'g' => 'h',
        'w' => 'x',
        other => panic!("unexpected canonical Base64 pad sextet {other}"),
    };
    bundle["signing"]["signature"] = Value::String(chars.into_iter().collect());
    let report = v2::verify_v2_document(
        &canonical(&bundle),
        Some(&proof.trust_store),
        Some(&proof.root),
    );
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "v2 structure");
}

#[test]
fn schema_requires_canonical_ed25519_signature_padding() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/contracts/evidence-bundle-v2.schema.json"
    ))
    .unwrap();
    assert_eq!(
        schema["$defs"]["signing"]["properties"]["signature"]["pattern"],
        "^[A-Za-z0-9+/]{85}[AQgw]==$"
    );
}

#[test]
fn v2_rejects_present_null_receipt_option() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| receipt["outcome"]["outcome_id"] = Value::Null,
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_rejected_status_with_unknown_outcome() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| receipt["status"] = Value::String("rejected".into()),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_accepts_nullable_sidecar_optionals() {
    let mut proof = fixture();
    let mut replacements = BTreeMap::new();
    for (path, field) in [
        ("lineage/control-task.json", "intent"),
        ("lineage/control-plan.json", "policy_decision_ref"),
    ] {
        let (old, new) = replace_json_artifact(&mut proof, path, |document| {
            document[field] = Value::Null;
        });
        replacements.insert(old, new);
    }
    mutate_control_receipt(
        &mut proof,
        |receipt| rewrite_refs(receipt, &replacements),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);

    let mut proof = fixture();
    let (outcome_digest, acceptance_digest) = add_control_outcome(&mut proof, "task-control");
    let (old, new) = replace_json_artifact(&mut proof, "outcomes/control.json", |document| {
        document["quality_score_milli"] = Value::Null;
        document["contract_ref"] = Value::Null;
    });
    assert_eq!(old, outcome_digest);
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["outcome"] = serde_json::json!({
                "state": "accepted",
                "outcome_id": "outcome-control",
                "outcome_ref": new.clone(),
                "acceptance_evidence_digest": acceptance_digest
            });
            receipt["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "kind": "outcome",
                    "uri": "artifact://control/outcome",
                "digest": new,
                    "media_type": "application/json",
                    "signature_status": "Verified"
                }));
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);
}

#[test]
fn v2_accepts_protocol_unknown_task_complexity() {
    let mut proof = fixture();
    let (old, new) = replace_json_artifact(&mut proof, "lineage/control-task.json", |document| {
        document["complexity"] = Value::String("unknown".into())
    });
    mutate_control_receipt(
        &mut proof,
        |receipt| rewrite_refs(receipt, &BTreeMap::from([(old, new)])),
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);
}

#[test]
fn v2_accepts_opaque_capability_id() {
    let mut proof = fixture();
    let (old_plan, new_plan) =
        replace_json_artifact(&mut proof, "lineage/control-plan.json", |document| {
            document["capability_ids"] = serde_json::json!(["capability:search"])
        });
    let (old_invocation, new_invocation) =
        replace_json_artifact(&mut proof, "lineage/control-invocation.json", |document| {
            document["operation"]["capability_id"] = Value::String("capability:search".into());
        });
    let replacements = BTreeMap::from([(old_plan, new_plan), (old_invocation, new_invocation)]);
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            rewrite_refs(receipt, &replacements);
            receipt["lineage"]["capabilities"][0]["capability_id"] =
                Value::String("capability:search".into());
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);
}

#[test]
fn v2_rejects_unsafe_receipt_evidence_uri() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["evidence_refs"][0]["uri"] = Value::String("artifact:///tmp/private".into());
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_invalid_receipt_media_type() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["evidence_refs"][0]["media_type"] = Value::String("text;plain/json".into());
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_invalid_receipt_signer_alias() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| receipt["signer"]["key_id"] = Value::String(" bad key".into()),
        |trust| {
            trust["trusted_signers"][0]["receipt_key_ids"] = serde_json::json!([" bad key"]);
        },
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "external signer trust");
}

#[test]
fn v2_rejects_oversized_receipt_semver() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["lineage"]["capabilities"][0]["capability_version"] =
                Value::String(format!("1.2.3+{}", "a".repeat(128)));
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_rejects_inconsistent_receipt_value_provenance() {
    let mut proof = fixture();
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["values"][0]["classification"] = Value::String("calculated".into());
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_verifies_exact_receipt_predecessor_chain() {
    let mut proof = fixture();
    let (previous_receipt_id, previous_signature_digest) = add_control_predecessor(&mut proof);
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["chain"]["sequence_number"] = Value::from(2_u64);
            receipt["chain"]["previous_receipt_id"] = Value::String(previous_receipt_id.clone());
            receipt["chain"]["previous_signature_digest"] =
                Value::String(previous_signature_digest.clone());
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(report.valid, "{:?}", report.steps);
}

#[test]
fn v2_rejects_receipt_predecessor_signature_fork() {
    let mut proof = fixture();
    let (previous_receipt_id, _) = add_control_predecessor(&mut proof);
    mutate_control_receipt(
        &mut proof,
        |receipt| {
            receipt["chain"]["sequence_number"] = Value::from(2_u64);
            receipt["chain"]["previous_receipt_id"] = Value::String(previous_receipt_id);
            receipt["chain"]["previous_signature_digest"] =
                Value::String(format!("sha256:{}", "0".repeat(64)));
        },
        |_| {},
    );
    let report = v2::verify_v2_document(&proof.bundle, Some(&proof.trust_store), Some(&proof.root));
    assert!(!report.valid);
    assert_eq!(report.steps[0].name, "signed arm receipts");
}

#[test]
fn v2_cli_proves_external_trust_and_rejects_tampered_sidecars() {
    let proof = fixture();
    let bundle_path = proof.root.join("customer-proof.json");
    let trust_store_path = proof.root.join("trust-store.json");
    fs::write(&bundle_path, &proof.bundle).unwrap();
    fs::write(&trust_store_path, &proof.trust_store).unwrap();

    let verify = || {
        Command::new(env!("CARGO_BIN_EXE_leanctx-verify"))
            .args([
                "v2",
                bundle_path.to_str().unwrap(),
                "--trust-store",
                trust_store_path.to_str().unwrap(),
                "--artifact-root",
                proof.root.to_str().unwrap(),
                "--json",
            ])
            .output()
            .expect("standalone verifier binary must execute")
    };

    let output = verify();
    assert!(
        output.status.success(),
        "expected valid V2 proof: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], Value::Bool(true));
    assert_eq!(report["proof_eligible"], Value::Bool(true));

    fs::write(proof.root.join("arms/control.json"), b"tampered").unwrap();
    let output = verify();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], Value::Bool(false));
    assert_eq!(
        report["steps"][0]["name"],
        Value::String("artifact inventory".into())
    );
}
