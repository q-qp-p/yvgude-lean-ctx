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
use std::sync::atomic::{AtomicUsize, Ordering};

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

fn write_artifact(root: &Path, path: &str) -> Vec<u8> {
    let bytes = format!("proof-artifact:{path}").into_bytes();
    let destination = root.join(path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, &bytes).unwrap();
    bytes
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
    let mut replacements = BTreeMap::new();
    let mut total_bytes = 0_u64;
    for item in bundle["inventory"]["items"].as_array_mut().unwrap() {
        let previous = item["ref"].as_str().unwrap().to_owned();
        let path = item["path"].as_str().unwrap().to_owned();
        let bytes = write_artifact(&root, &path);
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

    let key = SigningKey::from_bytes(&[17; 32]);
    let key_id = format!("id:{}", digest(key.verifying_key().as_bytes()));
    bundle["signing"]["key_id"] = Value::String(key_id.clone());
    bundle["signing"]["trusted_signer_ref"] = Value::String(format!("signer:{key_id}"));
    bundle["signing"]["trust_basis"] = Value::String("out_of_band".into());
    sign(&mut bundle, &key);
    let trust_store = serde_json::json!({
        "schema_version": "leanctx.customer-proof-trust-store/v1",
        "trusted_signers": [{
            "trusted_signer_ref": format!("signer:{key_id}"),
            "key_id": key_id,
            "public_key": hex(key.verifying_key().as_bytes()),
            "allowed_trust_bases": ["out_of_band"]
        }]
    });
    ProofFixture {
        root,
        bundle: canonical(&bundle),
        trust_store: canonical(&trust_store),
    }
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
