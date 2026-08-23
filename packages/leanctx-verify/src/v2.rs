//! Independent verifier for the customer-proof evidence-bundle-v2 contract.
//!
//! This deliberately shares no engine code. V2 is a canonical JSON document
//! with local sidecar artifacts and externally supplied trust; it is not a V1
//! ZIP archive and never accepts a self-attested key.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::de::{self, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use super::receipt::{verify_receipt_document, InventoryArtifact, VerifiedReceipt};
use super::verify::{hex_decode, sha256_hex, Step, StepStatus};

const BUNDLE_SCHEMA: &str = "leanctx.customer-proof-evidence-bundle/v2";
const TRUST_SCHEMA: &str = "leanctx.customer-proof-trust-store/v1";
const MAX_ITEMS: usize = 128;
const MAX_ITEM_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct V2Report {
    pub valid: bool,
    pub proof_eligible: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrustStore {
    schema_version: String,
    trust_revision: u64,
    evaluated_at: String,
    trusted_signers: Vec<TrustedSigner>,
    receipt_chain_heads: Vec<TrustedReceiptChainHead>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedReceiptChainHead {
    chain_id: String,
    sequence_number: u64,
    receipt_id: String,
}

struct VerifiedInventory {
    detail: String,
    artifacts: BTreeMap<String, InventoryArtifact>,
    refs: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrustedSigner {
    trusted_signer_ref: String,
    key_id: String,
    public_key: String,
    allowed_trust_bases: Vec<String>,
    receipt_key_ids: Vec<String>,
    revision: u64,
    admitted_at: String,
    expires_at: Option<String>,
    revoked_at: Option<String>,
}

#[derive(Clone)]
enum StrictJson {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl StrictJson {
    fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
            Self::Array(values) => Value::Array(values.into_iter().map(Self::into_value).collect()),
            Self::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, value.into_value()))
                    .collect(),
            ),
        }
    }
}

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrictVisitor;

        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictJson;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("strict JSON without duplicate object keys or floats")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictJson::Null)
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictJson::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictJson::Number(Number::from(value)))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictJson::Number(Number::from(value)))
            }

            fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Err(E::custom("floating-point JSON numbers are forbidden"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictJson::String(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictJson::String(value))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictJson>()? {
                    values.push(value);
                }
                Ok(StrictJson::Array(values))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(A::Error::custom(format!("duplicate JSON key '{key}'")));
                    }
                    values.insert(key, map.next_value::<StrictJson>()?);
                }
                Ok(StrictJson::Object(values))
            }
        }

        deserializer.deserialize_any(StrictVisitor)
    }
}

/// Verify an externally trusted V2 proof document and its sidecar artifacts.
pub fn verify_v2_document(
    raw: &[u8],
    trust_store_raw: Option<&[u8]>,
    artifact_root: Option<&Path>,
) -> V2Report {
    let mut steps = Vec::new();
    let document = match parse_canonical_json(raw, "bundle") {
        Ok(value) => value,
        Err(detail) => return failed_report("canonical JSON", detail),
    };
    if let Err(detail) = validate_shape(&document) {
        return failed_report("v2 structure", detail);
    }
    steps.push(passed(
        "canonical JSON + v2 structure",
        "canonical strict V2 document",
    ));

    let unsigned = match unsigned_document(&document) {
        Ok(value) => value,
        Err(detail) => return failed_report("bundle identity", detail),
    };
    let unsigned_bytes = canonical_bytes(&unsigned);
    let digest = format!("sha256:{}", sha256_hex(&unsigned_bytes));
    if let Err(detail) = verify_bundle_identity(&document, &digest) {
        return failed_report("bundle identity", detail);
    }
    steps.push(passed(
        "bundle identity",
        "canonical digest, ID, and signed digest match",
    ));

    let inventory = match verify_inventory(&document, artifact_root) {
        Ok(detail) => detail,
        Err(detail) => return failed_report("artifact inventory", detail),
    };
    steps.push(passed("artifact inventory", inventory.detail.clone()));

    if let Err(detail) = verify_semantics(&document) {
        return failed_report("semantic joins", detail);
    }
    steps.push(passed(
        "semantic joins",
        "matched arms, refs, quality, replay, and claims agree",
    ));

    let trust_store = match trust_store_raw {
        Some(raw) => match parse_trust_store(raw) {
            Ok(store) => store,
            Err(detail) => return failed_report("external signer trust", detail),
        },
        None => {
            return failed_report(
                "external signer trust",
                "--trust-store is required for V2 proof eligibility".to_string(),
            )
        }
    };
    let receipt_detail = match verify_arm_receipts(&document, &inventory, &trust_store) {
        Ok(detail) => detail,
        Err(detail) => return failed_report("signed arm receipts", detail),
    };
    steps.push(passed("signed arm receipts", receipt_detail));

    let key = match trusted_key(&document, &trust_store) {
        Ok(key) => key,
        Err(detail) => return failed_report("external signer trust", detail),
    };
    steps.push(passed(
        "external signer trust",
        "one external trusted signer matched key identity and trust basis",
    ));

    if let Err(detail) = verify_signature(&document, &unsigned_bytes, &key) {
        return failed_report("Ed25519 signature", detail);
    }
    steps.push(passed(
        "Ed25519 signature",
        "external trusted key verifies canonical unsigned bytes",
    ));

    V2Report {
        valid: true,
        proof_eligible: true,
        steps,
    }
}

fn failed_report(name: &'static str, detail: String) -> V2Report {
    V2Report {
        valid: false,
        proof_eligible: false,
        steps: vec![Step {
            name,
            status: StepStatus::Fail,
            detail,
        }],
    }
}

fn passed(name: &'static str, detail: impl Into<String>) -> Step {
    Step {
        name,
        status: StepStatus::Pass,
        detail: detail.into(),
    }
}

pub(crate) fn parse_canonical_json(raw: &[u8], label: &str) -> Result<Value, String> {
    let parsed: StrictJson = serde_json::from_slice(raw)
        .map_err(|error| format!("{label} is not strict JSON: {error}"))?;
    let value = parsed.into_value();
    let canonical = canonical_bytes(&value);
    if raw != canonical {
        return Err(format!(
            "{label} is not canonical compact sorted UTF-8 JSON"
        ));
    }
    Ok(value)
}

pub(crate) fn canonical_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(&sort_value(value.clone())).expect("JSON value always serializes")
}

fn sort_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(sort_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, sort_value(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        scalar => scalar,
    }
}

pub(crate) fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

pub(crate) fn array<'a>(value: &'a Value, label: &str) -> Result<&'a [Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{label} must be an array"))
}

pub(crate) fn field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a Value, String> {
    object
        .get(key)
        .ok_or_else(|| format!("{label}.{key} is required"))
}

pub(crate) fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    field(object, key, label)?
        .as_str()
        .ok_or_else(|| format!("{label}.{key} must be a string"))
}

pub(crate) fn unsigned(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
    max: u64,
) -> Result<u64, String> {
    field(object, key, label)?
        .as_u64()
        .filter(|value| *value <= max)
        .ok_or_else(|| format!("{label}.{key} must be an integer in range"))
}

pub(crate) fn check_fields(
    object: &Map<String, Value>,
    label: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<(), String> {
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("{label}.{key} is required"));
        }
    }
    if let Some(unknown) = object
        .keys()
        .find(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(format!("{label} has unknown field '{unknown}'"));
    }
    Ok(())
}

fn in_set(value: &str, allowed: &[&str], label: &str) -> Result<(), String> {
    allowed
        .contains(&value)
        .then_some(())
        .ok_or_else(|| format!("{label} has unsupported value '{value}'"))
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn is_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| is_hex(hex, 64))
}

pub(crate) fn is_content_id(value: &str) -> bool {
    value
        .strip_prefix("id:sha256:")
        .is_some_and(|hex| is_hex(hex, 64))
}

fn is_signer_ref(value: &str) -> bool {
    value
        .strip_prefix("signer:id:sha256:")
        .is_some_and(|hex| is_hex(hex, 64))
}

fn is_safe_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && !Path::new(value).is_absolute()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(crate) fn is_rfc3339_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || !bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19))
            .all(|(_, byte)| byte.is_ascii_digit())
    {
        return false;
    }
    let parse = |range: std::ops::Range<usize>| -> Option<u32> {
        std::str::from_utf8(&bytes[range]).ok()?.parse().ok()
    };
    let Some(year) = parse(0..4) else {
        return false;
    };
    let Some(month) = parse(5..7) else {
        return false;
    };
    let Some(day) = parse(8..10) else {
        return false;
    };
    let Some(hour) = parse(11..13) else {
        return false;
    };
    let Some(minute) = parse(14..16) else {
        return false;
    };
    let Some(second) = parse(17..19) else {
        return false;
    };
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    year != 0 && (1..=days_in_month).contains(&day) && hour < 24 && minute < 60 && second < 60
}

fn validate_shape(document: &Value) -> Result<(), String> {
    let root = object(document, "bundle")?;
    check_fields(
        root,
        "bundle",
        &[
            "schema_version",
            "bundle_kind",
            "bundle_id",
            "bundle_digest",
            "created_at",
            "status",
            "subject",
            "matched_arms",
            "inventory",
            "quality",
            "replay",
            "limitations",
            "redaction",
            "claims",
            "signing",
        ],
        &[],
    )?;
    if string(root, "schema_version", "bundle")? != BUNDLE_SCHEMA
        || string(root, "bundle_kind", "bundle")? != "customer-proof"
    {
        return Err("bundle schema_version or bundle_kind is unsupported".to_string());
    }
    if !is_content_id(string(root, "bundle_id", "bundle")?)
        || !is_digest(string(root, "bundle_digest", "bundle")?)
    {
        return Err("bundle_id or bundle_digest is malformed".to_string());
    }
    if !is_rfc3339_utc_timestamp(string(root, "created_at", "bundle")?) {
        return Err("bundle.created_at must be an RFC 3339 UTC timestamp".to_string());
    }
    in_set(
        string(root, "status", "bundle")?,
        &["complete", "partial", "invalid"],
        "bundle.status",
    )?;
    validate_subject(field(root, "subject", "bundle")?)?;
    validate_arms(field(root, "matched_arms", "bundle")?)?;
    validate_inventory(field(root, "inventory", "bundle")?)?;
    validate_quality(field(root, "quality", "bundle")?)?;
    validate_replay(field(root, "replay", "bundle")?)?;
    validate_limitations(field(root, "limitations", "bundle")?)?;
    validate_redaction(field(root, "redaction", "bundle")?)?;
    validate_claims(field(root, "claims", "bundle")?)?;
    validate_signing(field(root, "signing", "bundle")?)
}

fn validate_subject(value: &Value) -> Result<(), String> {
    let subject = object(value, "subject")?;
    check_fields(
        subject,
        "subject",
        &["customer_ref", "project_ref", "workload_ref"],
        &[],
    )?;
    let customer = string(subject, "customer_ref", "subject")?;
    let project = string(subject, "project_ref", "subject")?;
    if !restricted_ref(customer, "customer:", 2, 64)
        || !restricted_ref(project, "project:", 2, 64)
        || !is_content_id(string(subject, "workload_ref", "subject")?)
    {
        return Err("subject references are malformed".to_string());
    }
    Ok(())
}

fn validate_identity(value: &Value, label: &str) -> Result<(), String> {
    let identity = object(value, label)?;
    check_fields(
        identity,
        label,
        &["provider", "model", "source_commit", "workload_digest"],
        &["endpoint_ref"],
    )?;
    if !restricted_name(string(identity, "provider", label)?, 2, 64, false)
        || !restricted_name(string(identity, "model", label)?, 1, 128, true)
        || !string(identity, "source_commit", label)?
            .strip_prefix("git:")
            .is_some_and(|hex| is_hex(hex, 40) || is_hex(hex, 64))
        || !is_digest(string(identity, "workload_digest", label)?)
    {
        return Err(format!("{label} is malformed"));
    }
    if identity.contains_key("endpoint_ref")
        && !restricted_name(string(identity, "endpoint_ref", label)?, 1, 128, true)
    {
        return Err(format!("{label}.endpoint_ref is malformed"));
    }
    Ok(())
}

fn restricted_ref(value: &str, prefix: &str, min: usize, max: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| restricted_name(suffix, min, max, false))
}

fn restricted_name(value: &str, min: usize, max: usize, extended: bool) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= min
        && bytes.len() <= max
        && bytes[0].is_ascii_alphanumeric()
        && (!bytes[0].is_ascii_uppercase() || extended)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-')
                || (extended && (byte.is_ascii_uppercase() || matches!(byte, b':' | b'/' | b'@')))
        })
}

fn validate_measurements(value: &Value, label: &str) -> Result<(), String> {
    let measurements = object(value, label)?;
    check_fields(
        measurements,
        label,
        &[
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "latency_ms",
            "cost",
            "status",
        ],
        &[],
    )?;
    for key in [
        "input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "latency_ms",
    ] {
        let _ = unsigned(measurements, key, label, i64::MAX as u64)?;
    }
    in_set(
        string(measurements, "status", label)?,
        &["observed", "estimated", "unavailable", "not_applicable"],
        &format!("{label}.status"),
    )?;
    let cost = object(
        field(measurements, "cost", label)?,
        &format!("{label}.cost"),
    )?;
    check_fields(
        cost,
        &format!("{label}.cost"),
        &["currency", "amount_micros", "status"],
        &[],
    )?;
    let currency = string(cost, "currency", &format!("{label}.cost"))?;
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(format!("{label}.cost.currency is malformed"));
    }
    let _ = unsigned(
        cost,
        "amount_micros",
        &format!("{label}.cost"),
        i64::MAX as u64,
    )?;
    in_set(
        string(cost, "status", &format!("{label}.cost"))?,
        &["observed", "estimated", "unavailable", "not_applicable"],
        &format!("{label}.cost.status"),
    )
}

fn validate_arm(value: &Value, label: &str, expected_role: &str) -> Result<(), String> {
    let arm = object(value, label)?;
    check_fields(
        arm,
        label,
        &[
            "role",
            "arm_id",
            "identity",
            "status",
            "measurements",
            "evidence_refs",
        ],
        &[],
    )?;
    if string(arm, "role", label)? != expected_role || !is_content_id(string(arm, "arm_id", label)?)
    {
        return Err(format!("{label} role or arm_id is invalid"));
    }
    in_set(
        string(arm, "status", label)?,
        &["complete", "partial", "failed"],
        &format!("{label}.status"),
    )?;
    validate_identity(field(arm, "identity", label)?, &format!("{label}.identity"))?;
    validate_measurements(
        field(arm, "measurements", label)?,
        &format!("{label}.measurements"),
    )?;
    validate_refs(
        field(arm, "evidence_refs", label)?,
        &format!("{label}.evidence_refs"),
        1,
        32,
    )
}

fn validate_arms(value: &Value) -> Result<(), String> {
    let arms = object(value, "matched_arms")?;
    check_fields(
        arms,
        "matched_arms",
        &[
            "match_id",
            "match_basis",
            "shared_identity",
            "control",
            "treatment",
        ],
        &[],
    )?;
    if !is_content_id(string(arms, "match_id", "matched_arms")?) {
        return Err("matched_arms.match_id is malformed".to_string());
    }
    let basis = array(
        field(arms, "match_basis", "matched_arms")?,
        "matched_arms.match_basis",
    )?;
    let expected = ["provider", "model", "source_commit", "workload_digest"];
    if basis.len() != expected.len()
        || !basis
            .iter()
            .zip(expected)
            .all(|(value, expected)| value.as_str() == Some(expected))
    {
        return Err("matched_arms.match_basis must be the exact canonical basis".to_string());
    }
    validate_identity(
        field(arms, "shared_identity", "matched_arms")?,
        "matched_arms.shared_identity",
    )?;
    validate_arm(
        field(arms, "control", "matched_arms")?,
        "matched_arms.control",
        "control",
    )?;
    validate_arm(
        field(arms, "treatment", "matched_arms")?,
        "matched_arms.treatment",
        "treatment",
    )
}

fn validate_inventory(value: &Value) -> Result<(), String> {
    let inventory = object(value, "inventory")?;
    check_fields(
        inventory,
        "inventory",
        &["max_items", "item_count", "total_bytes", "items"],
        &[],
    )?;
    if unsigned(inventory, "max_items", "inventory", MAX_ITEMS as u64)? != MAX_ITEMS as u64 {
        return Err("inventory.max_items must be 128".to_string());
    }
    let items = array(field(inventory, "items", "inventory")?, "inventory.items")?;
    if items.len() > MAX_ITEMS
        || unsigned(inventory, "item_count", "inventory", MAX_ITEMS as u64)? != items.len() as u64
    {
        return Err("inventory.item_count is invalid".to_string());
    }
    let mut refs = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut total = 0_u64;
    for (index, item) in items.iter().enumerate() {
        let label = format!("inventory.items[{index}]");
        let item = object(item, &label)?;
        check_fields(
            item,
            &label,
            &[
                "ref",
                "kind",
                "path",
                "digest",
                "size_bytes",
                "availability",
                "redaction_class",
            ],
            &[],
        )?;
        let reference = string(item, "ref", &label)?;
        let digest = string(item, "digest", &label)?;
        if !is_content_id(reference) || !is_digest(digest) || reference != format!("id:{digest}") {
            return Err(format!("{label} ref and digest do not match"));
        }
        in_set(
            string(item, "kind", &label)?,
            &[
                "arm_receipt",
                "receipt_predecessor",
                "quality_measurement",
                "replay_input",
                "replay_result",
                "run_metadata",
                "claim_basis",
                "frozen_audit_bundle_v1",
                "task_envelope",
                "execution_plan",
                "engine_invocation",
                "accepted_outcome",
                "measurement",
                "assumption",
                "formula",
                "price_table",
                "invoice",
                "acceptance_evidence",
            ],
            &format!("{label}.kind"),
        )?;
        let path = string(item, "path", &label)?;
        if !is_safe_path(path)
            || !refs.insert(reference.to_owned())
            || !paths.insert(path.to_owned())
        {
            return Err(format!("{label} has an unsafe or duplicate ref/path"));
        }
        let bytes = unsigned(item, "size_bytes", &label, MAX_ITEM_BYTES)?;
        total = total
            .checked_add(bytes)
            .ok_or_else(|| "inventory total overflows".to_string())?;
        in_set(
            string(item, "availability", &label)?,
            &["present", "omitted", "unavailable"],
            &format!("{label}.availability"),
        )?;
        in_set(
            string(item, "redaction_class", &label)?,
            &[
                "none",
                "pseudonymized",
                "metadata_only",
                "content_removed",
                "secret_removed",
                "aggregated",
            ],
            &format!("{label}.redaction_class"),
        )?;
    }
    if total > MAX_TOTAL_BYTES
        || unsigned(inventory, "total_bytes", "inventory", MAX_TOTAL_BYTES)? != total
    {
        return Err("inventory.total_bytes is invalid".to_string());
    }
    Ok(())
}

fn validate_refs(value: &Value, label: &str, min: usize, max: usize) -> Result<(), String> {
    let values = array(value, label)?;
    if values.len() < min || values.len() > max {
        return Err(format!("{label} has an invalid item count"));
    }
    let mut refs = BTreeSet::new();
    for value in values {
        let reference = value
            .as_str()
            .ok_or_else(|| format!("{label} must contain strings"))?;
        if !is_content_id(reference) || !refs.insert(reference) {
            return Err(format!("{label} has an invalid or duplicate reference"));
        }
    }
    Ok(())
}

fn validate_quality(value: &Value) -> Result<(), String> {
    let quality = object(value, "quality")?;
    check_fields(
        quality,
        "quality",
        &[
            "status",
            "metric",
            "control_score_milli",
            "treatment_score_milli",
            "confidence",
            "method",
            "evidence_refs",
        ],
        &[],
    )?;
    in_set(
        string(quality, "status", "quality")?,
        &["preserved", "degraded", "inconclusive", "not_measured"],
        "quality.status",
    )?;
    if string(quality, "metric", "quality")? != "score_milli" {
        return Err("quality.metric must be score_milli".to_string());
    }
    let _ = unsigned(quality, "control_score_milli", "quality", 1_000)?;
    let _ = unsigned(quality, "treatment_score_milli", "quality", 1_000)?;
    in_set(
        string(quality, "confidence", "quality")?,
        &["high", "medium", "low", "unavailable"],
        "quality.confidence",
    )?;
    in_set(
        string(quality, "method", "quality")?,
        &["human", "automated", "mixed", "unavailable"],
        "quality.method",
    )?;
    validate_refs(
        field(quality, "evidence_refs", "quality")?,
        "quality.evidence_refs",
        1,
        32,
    )
}

fn validate_replay(value: &Value) -> Result<(), String> {
    let replay = object(value, "replay")?;
    check_fields(
        replay,
        "replay",
        &[
            "status",
            "mode",
            "determinism",
            "input_refs",
            "result_refs",
            "notes",
        ],
        &[],
    )?;
    in_set(
        string(replay, "status", "replay")?,
        &["replayable", "partial", "not_replayable", "not_attempted"],
        "replay.status",
    )?;
    in_set(
        string(replay, "mode", "replay")?,
        &["offline", "online", "mixed", "none"],
        "replay.mode",
    )?;
    in_set(
        string(replay, "determinism", "replay")?,
        &[
            "deterministic",
            "same_inputs_expected",
            "not_deterministic",
            "not_assessed",
        ],
        "replay.determinism",
    )?;
    bounded_chars(string(replay, "notes", "replay")?, 0, 1024, "replay.notes")?;
    validate_refs(
        field(replay, "input_refs", "replay")?,
        "replay.input_refs",
        0,
        MAX_ITEMS,
    )?;
    validate_refs(
        field(replay, "result_refs", "replay")?,
        "replay.result_refs",
        0,
        MAX_ITEMS,
    )
}

fn validate_limitations(value: &Value) -> Result<(), String> {
    let limitations = object(value, "limitations")?;
    check_fields(
        limitations,
        "limitations",
        &["known_limitations", "unproven"],
        &[],
    )?;
    let known = array(
        field(limitations, "known_limitations", "limitations")?,
        "limitations.known_limitations",
    )?;
    let unproven = array(
        field(limitations, "unproven", "limitations")?,
        "limitations.unproven",
    )?;
    if known.len() > 32 || unproven.is_empty() || unproven.len() > 32 {
        return Err("limitations item count is invalid".to_string());
    }
    let mut known_values = BTreeSet::new();
    for value in known {
        let value = value
            .as_str()
            .ok_or_else(|| "limitations.known_limitations must contain strings".to_string())?;
        bounded_chars(value, 0, 512, "limitations.known_limitations item")?;
        if !known_values.insert(value) {
            return Err("limitations.known_limitations must be unique".to_string());
        }
    }
    let mut unproven_values = BTreeSet::new();
    for value in unproven {
        let value = value
            .as_str()
            .ok_or_else(|| "limitations.unproven must contain strings".to_string())?;
        in_set(
            value,
            &[
                "omission_before_capture",
                "third_party_attestation",
                "generalization_beyond_workload",
                "production_sla",
                "future_outcomes",
                "unavailable_external_service",
                "redacted_content",
            ],
            "limitations.unproven item",
        )?;
        if !unproven_values.insert(value) {
            return Err("limitations.unproven must be unique".to_string());
        }
    }
    Ok(())
}

fn validate_redaction(value: &Value) -> Result<(), String> {
    let redaction = object(value, "redaction")?;
    check_fields(
        redaction,
        "redaction",
        &["class", "policy", "reversible", "notes"],
        &[],
    )?;
    in_set(
        string(redaction, "class", "redaction")?,
        &[
            "none",
            "pseudonymized",
            "metadata_only",
            "content_removed",
            "secret_removed",
            "aggregated",
        ],
        "redaction.class",
    )?;
    in_set(
        string(redaction, "policy", "redaction")?,
        &[
            "no_redaction",
            "customer_policy",
            "secret_scrub",
            "content_minimization",
        ],
        "redaction.policy",
    )?;
    field(redaction, "reversible", "redaction")?
        .as_bool()
        .ok_or_else(|| "redaction.reversible must be boolean".to_string())?;
    bounded_chars(
        string(redaction, "notes", "redaction")?,
        0,
        512,
        "redaction.notes",
    )?;
    Ok(())
}

fn validate_claims(value: &Value) -> Result<(), String> {
    let claims = array(value, "claims")?;
    if claims.is_empty() || claims.len() > 32 {
        return Err("claims item count is invalid".to_string());
    }
    let mut ids = BTreeSet::new();
    for (index, claim) in claims.iter().enumerate() {
        let label = format!("claims[{index}]");
        let claim = object(claim, &label)?;
        check_fields(
            claim,
            &label,
            &[
                "claim_id",
                "claim_type",
                "statement",
                "claim_validity",
                "scope",
                "basis_refs",
            ],
            &[],
        )?;
        let id = string(claim, "claim_id", &label)?;
        if !is_content_id(id) || !ids.insert(id) {
            return Err(format!("{label} id or statement is invalid"));
        }
        bounded_chars(
            string(claim, "statement", &label)?,
            1,
            512,
            &format!("{label}.statement"),
        )?;
        in_set(
            string(claim, "claim_type", &label)?,
            &[
                "cost_reduction",
                "token_reduction",
                "quality_preserved",
                "latency_change",
                "replayability",
                "other",
            ],
            &format!("{label}.claim_type"),
        )?;
        in_set(
            string(claim, "claim_validity", &label)?,
            &["supported", "inconclusive", "unsupported", "not_asserted"],
            &format!("{label}.claim_validity"),
        )?;
        in_set(
            string(claim, "scope", &label)?,
            &["matched_run", "customer_workload", "general"],
            &format!("{label}.scope"),
        )?;
        validate_refs(
            field(claim, "basis_refs", &label)?,
            &format!("{label}.basis_refs"),
            1,
            32,
        )?;
    }
    Ok(())
}

fn bounded_chars(value: &str, min: usize, max: usize, label: &str) -> Result<(), String> {
    let count = value.chars().count();
    if count < min || count > max {
        return Err(format!("{label} has an invalid length"));
    }
    Ok(())
}

fn validate_signing(value: &Value) -> Result<(), String> {
    let signing = object(value, "signing")?;
    check_fields(
        signing,
        "signing",
        &[
            "algorithm",
            "trusted_signer_ref",
            "key_id",
            "trust_basis",
            "signed_digest",
            "signature",
        ],
        &[],
    )?;
    if string(signing, "algorithm", "signing")? != "Ed25519"
        || !is_signer_ref(string(signing, "trusted_signer_ref", "signing")?)
        || !is_content_id(string(signing, "key_id", "signing")?)
        || !is_digest(string(signing, "signed_digest", "signing")?)
    {
        return Err("signing identity is malformed".to_string());
    }
    in_set(
        string(signing, "trust_basis", "signing")?,
        &["customer_configured", "out_of_band", "local_identity"],
        "signing.trust_basis",
    )?;
    let encoded = string(signing, "signature", "signing")?;
    let signature = STANDARD
        .decode(encoded)
        .map_err(|_| "signing.signature is not standard Base64".to_string())?;
    if signature.len() != 64 || STANDARD.encode(&signature) != encoded {
        return Err("signing.signature is not canonical Ed25519 Base64".to_string());
    }
    Ok(())
}

fn unsigned_document(document: &Value) -> Result<Value, String> {
    let mut unsigned = document.clone();
    let root = unsigned
        .as_object_mut()
        .ok_or_else(|| "bundle must be an object".to_string())?;
    root.remove("bundle_id");
    root.remove("bundle_digest");
    let signing = root
        .get_mut("signing")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "bundle.signing must be an object".to_string())?;
    signing.remove("signed_digest");
    signing.remove("signature");
    Ok(unsigned)
}

fn verify_bundle_identity(document: &Value, digest: &str) -> Result<(), String> {
    let root = object(document, "bundle")?;
    if string(root, "bundle_digest", "bundle")? != digest
        || string(root, "bundle_id", "bundle")? != format!("id:{digest}")
        || string(
            object(field(root, "signing", "bundle")?, "signing")?,
            "signed_digest",
            "signing",
        )? != digest
    {
        return Err("bundle digest, bundle ID, or signed digest does not recompute".to_string());
    }
    Ok(())
}

fn verify_inventory(
    document: &Value,
    artifact_root: Option<&Path>,
) -> Result<VerifiedInventory, String> {
    let root = object(document, "bundle")?;
    let bundle_inventory = object(field(root, "inventory", "bundle")?, "inventory")?;
    let items = array(
        field(bundle_inventory, "items", "inventory")?,
        "inventory.items",
    )?;
    let root = artifact_root
        .ok_or_else(|| "artifact root is required for proof eligibility".to_string())?;
    let canonical_root =
        fs::canonicalize(root).map_err(|error| format!("artifact root is unavailable: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("artifact root is not a directory".to_string());
    }
    let mut artifacts = BTreeMap::new();
    let mut refs = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        let label = format!("inventory.items[{index}]");
        let item = object(item, &label)?;
        if string(item, "availability", &label)? != "present" {
            continue;
        }
        let artifact = safe_artifact_path(&canonical_root, string(item, "path", &label)?)?;
        let file =
            fs::File::open(&artifact).map_err(|error| format!("{label} is missing: {error}"))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("{label} metadata is unavailable: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err(format!("{label} is not a regular file"));
        }
        let expected_size = unsigned(item, "size_bytes", &label, MAX_ITEM_BYTES)?;
        if metadata.len() != expected_size {
            return Err(format!("{label} size does not match inventory"));
        }
        let capacity = usize::try_from(expected_size)
            .map_err(|_| format!("{label} size cannot be represented locally"))?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(expected_size.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{label} cannot be read: {error}"))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != expected_size {
            return Err(format!("{label} bytes changed while being read"));
        }
        let digest = string(item, "digest", &label)?;
        if format!("sha256:{}", sha256_hex(&bytes)) != digest {
            return Err(format!("{label} digest does not match artifact"));
        }
        if artifacts
            .insert(
                digest.to_owned(),
                InventoryArtifact {
                    kind: string(item, "kind", &label)?.to_owned(),
                    bytes,
                },
            )
            .is_some()
            || refs
                .insert(string(item, "ref", &label)?.to_owned(), digest.to_owned())
                .is_some()
        {
            return Err("present inventory digests and refs must be unique".to_string());
        }
    }
    Ok(VerifiedInventory {
        detail: format!(
            "{} inventory artifacts are bounded, local, and hash-verified",
            items.len()
        ),
        artifacts,
        refs,
    })
}

fn verify_arm_receipts(
    document: &Value,
    inventory: &VerifiedInventory,
    trust: &TrustStore,
) -> Result<String, String> {
    let root = object(document, "bundle")?;
    let bundle_inventory = object(field(root, "inventory", "bundle")?, "inventory")?;
    let items = array(
        field(bundle_inventory, "items", "inventory")?,
        "inventory.items",
    )?;
    let mut receipt_refs = BTreeSet::new();
    for item in items {
        let item = object(item, "inventory item")?;
        if string(item, "availability", "inventory item")? != "present" {
            continue;
        }
        let kind = string(item, "kind", "inventory item")?.to_owned();
        if kind == "arm_receipt" {
            receipt_refs.insert(string(item, "ref", "inventory item")?.to_owned());
        }
    }
    let arms = object(field(root, "matched_arms", "bundle")?, "matched_arms")?;
    let mut joined = BTreeSet::new();
    for arm_name in ["control", "treatment"] {
        let arm = object(field(arms, arm_name, "matched_arms")?, "arm")?;
        let matching: Vec<&str> = array(field(arm, "evidence_refs", "arm")?, "arm.evidence_refs")?
            .iter()
            .filter_map(Value::as_str)
            .filter(|reference| receipt_refs.contains(*reference))
            .collect();
        if matching.len() != 1 || !joined.insert(matching[0]) {
            return Err(format!("{arm_name} must join one distinct arm_receipt"));
        }
    }
    if receipt_refs.len() != joined.len() {
        return Err("unreferenced or missing arm receipt inventory item".to_string());
    }
    let mut verified = BTreeMap::<String, VerifiedReceipt>::new();
    let mut receipt_by_ref = BTreeMap::new();
    let mut predecessor_ids = BTreeSet::new();
    let mut positions = BTreeSet::new();
    for (digest, artifact) in &inventory.artifacts {
        if !matches!(
            artifact.kind.as_str(),
            "arm_receipt" | "receipt_predecessor"
        ) {
            continue;
        }
        let receipt = verify_receipt_document(&artifact.bytes, &inventory.artifacts, trust)?;
        if !positions.insert((receipt.chain_id.clone(), receipt.sequence_number))
            || verified.contains_key(&receipt.receipt_id)
        {
            return Err("receipt inventory contains a duplicate chain position or ID".to_string());
        }
        if artifact.kind == "arm_receipt" {
            let reference = inventory
                .refs
                .iter()
                .find_map(|(reference, candidate)| (candidate == digest).then_some(reference))
                .ok_or_else(|| "arm receipt has no verified inventory ref".to_string())?;
            receipt_by_ref.insert(reference.clone(), receipt.receipt_id.clone());
        } else {
            predecessor_ids.insert(receipt.receipt_id.clone());
        }
        verified.insert(receipt.receipt_id.clone(), receipt);
    }
    let mut verified_chains = BTreeSet::new();
    let mut reachable = BTreeSet::new();
    for reference in &joined {
        let receipt_id = receipt_by_ref
            .get(*reference)
            .ok_or_else(|| "arm receipt ref did not resolve to verified bytes".to_string())?;
        let receipt = verified
            .get(receipt_id)
            .ok_or_else(|| "arm receipt did not resolve to verified bytes".to_string())?;
        let heads: Vec<&TrustedReceiptChainHead> = trust
            .receipt_chain_heads
            .iter()
            .filter(|head| head.chain_id == receipt.chain_id)
            .collect();
        if heads.len() != 1
            || heads[0].sequence_number != receipt.sequence_number
            || heads[0].receipt_id != receipt.receipt_id
            || !verified_chains.insert(receipt.chain_id.clone())
        {
            return Err(
                "receipt is stale, forked, duplicated, or absent from trusted chain heads"
                    .to_string(),
            );
        }
        verify_receipt_chain(&verified, receipt, &mut reachable)?;
    }
    if predecessor_ids
        .iter()
        .any(|receipt_id| !reachable.contains(receipt_id))
    {
        return Err("receipt inventory contains an unreferenced predecessor".to_string());
    }
    Ok(format!(
        "{} canonical signed receipts resolve all lineage/evidence digests",
        joined.len()
    ))
}

fn verify_receipt_chain(
    receipts: &BTreeMap<String, VerifiedReceipt>,
    head: &VerifiedReceipt,
    reachable: &mut BTreeSet<String>,
) -> Result<(), String> {
    let mut current = head;
    loop {
        if !reachable.insert(current.receipt_id.clone()) {
            return Err("receipt chain contains a cycle or shared fork".to_string());
        }
        if current.sequence_number == 1 {
            return Ok(());
        }
        let previous_id = current
            .previous_receipt_id
            .as_deref()
            .ok_or_else(|| "non-genesis receipt omits predecessor ID".to_string())?;
        let previous = receipts
            .get(previous_id)
            .ok_or_else(|| "receipt predecessor bytes are absent from inventory".to_string())?;
        if previous.chain_id != current.chain_id
            || previous.sequence_number.checked_add(1) != Some(current.sequence_number)
            || current.previous_signature_digest.as_deref()
                != Some(previous.signature_digest.as_str())
        {
            return Err("receipt predecessor chain or signature binding is invalid".to_string());
        }
        current = previous;
    }
}

fn safe_artifact_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    if !is_safe_path(path) {
        return Err("inventory path is unsafe".to_string());
    }
    let resolved = fs::canonicalize(root.join(path))
        .map_err(|error| format!("artifact path cannot be resolved: {error}"))?;
    resolved
        .starts_with(root)
        .then_some(resolved)
        .ok_or_else(|| "artifact path escapes artifact root".to_string())
}

fn inventory_refs(document: &Value) -> Result<BTreeMap<String, String>, String> {
    let root = object(document, "bundle")?;
    let inventory = object(field(root, "inventory", "bundle")?, "inventory")?;
    let items = array(field(inventory, "items", "inventory")?, "inventory.items")?;
    items
        .iter()
        .map(|item| {
            let item = object(item, "inventory item")?;
            Ok((
                string(item, "ref", "inventory item")?.to_owned(),
                string(item, "availability", "inventory item")?.to_owned(),
            ))
        })
        .collect()
}

fn verify_semantics(document: &Value) -> Result<(), String> {
    let root = object(document, "bundle")?;
    let arms = object(field(root, "matched_arms", "bundle")?, "matched_arms")?;
    let shared = object(
        field(arms, "shared_identity", "matched_arms")?,
        "matched_arms.shared_identity",
    )?;
    for arm_name in ["control", "treatment"] {
        let arm = object(
            field(arms, arm_name, "matched_arms")?,
            &format!("matched_arms.{arm_name}"),
        )?;
        let identity = object(
            field(arm, "identity", &format!("matched_arms.{arm_name}"))?,
            "arm identity",
        )?;
        for key in ["provider", "model", "source_commit", "workload_digest"] {
            if field(identity, key, "arm identity")?
                != field(shared, key, "matched_arms.shared_identity")?
            {
                return Err(format!("{arm_name} identity does not match shared {key}"));
            }
        }
    }
    let subject = object(field(root, "subject", "bundle")?, "subject")?;
    if string(subject, "workload_ref", "subject")?
        != format!(
            "id:{}",
            string(shared, "workload_digest", "matched_arms.shared_identity")?
        )
    {
        return Err("subject workload_ref does not join matched workload_digest".to_string());
    }
    let refs = inventory_refs(document)?;
    let ensure_refs = |value: &Value, label: &str| -> Result<(), String> {
        for reference in array(value, label)? {
            let reference = reference
                .as_str()
                .ok_or_else(|| format!("{label} has non-string reference"))?;
            if refs
                .get(reference)
                .is_none_or(|availability| availability != "present")
            {
                return Err(format!(
                    "{label} references missing or non-present artifact"
                ));
            }
        }
        Ok(())
    };
    for arm_name in ["control", "treatment"] {
        let arm = object(field(arms, arm_name, "matched_arms")?, "arm")?;
        ensure_refs(field(arm, "evidence_refs", "arm")?, "arm.evidence_refs")?;
    }
    let quality = object(field(root, "quality", "bundle")?, "quality")?;
    ensure_refs(
        field(quality, "evidence_refs", "quality")?,
        "quality.evidence_refs",
    )?;
    let control_score = unsigned(quality, "control_score_milli", "quality", 1_000)?;
    let treatment_score = unsigned(quality, "treatment_score_milli", "quality", 1_000)?;
    match string(quality, "status", "quality")? {
        "preserved" if treatment_score < control_score => {
            return Err("quality preserved contradicts scores".to_string())
        }
        "degraded" if treatment_score >= control_score => {
            return Err("quality degraded contradicts scores".to_string())
        }
        "not_measured" if string(quality, "method", "quality")? != "unavailable" => {
            return Err("not_measured quality requires unavailable method".to_string())
        }
        _ => {}
    }
    let replay = object(field(root, "replay", "bundle")?, "replay")?;
    ensure_refs(field(replay, "input_refs", "replay")?, "replay.input_refs")?;
    ensure_refs(
        field(replay, "result_refs", "replay")?,
        "replay.result_refs",
    )?;
    let claims = array(field(root, "claims", "bundle")?, "claims")?;
    let complete = string(root, "status", "bundle")? == "complete";
    let control = object(field(arms, "control", "matched_arms")?, "control")?;
    let treatment = object(field(arms, "treatment", "matched_arms")?, "treatment")?;
    for claim in claims {
        let claim = object(claim, "claim")?;
        let supported = string(claim, "claim_validity", "claim")? == "supported";
        ensure_refs(field(claim, "basis_refs", "claim")?, "claim.basis_refs")?;
        if !supported {
            continue;
        }
        if !complete
            || string(control, "status", "control")? != "complete"
            || string(treatment, "status", "treatment")? != "complete"
        {
            return Err("supported claim requires complete bundle and arms".to_string());
        }
        if string(replay, "status", "replay")? == "partial"
            && string(claim, "scope", "claim")? != "matched_run"
        {
            return Err("partial replay cannot support a broad claim".to_string());
        }
        verify_supported_claim(claim, control, treatment, quality, replay)?;
    }
    Ok(())
}

fn verify_supported_claim(
    claim: &Map<String, Value>,
    control: &Map<String, Value>,
    treatment: &Map<String, Value>,
    quality: &Map<String, Value>,
    replay: &Map<String, Value>,
) -> Result<(), String> {
    let kind = string(claim, "claim_type", "claim")?;
    let control_measurements = object(
        field(control, "measurements", "control")?,
        "control.measurements",
    )?;
    let treatment_measurements = object(
        field(treatment, "measurements", "treatment")?,
        "treatment.measurements",
    )?;
    match kind {
        "cost_reduction" => {
            let control_cost = object(
                field(control_measurements, "cost", "control.measurements")?,
                "control.cost",
            )?;
            let treatment_cost = object(
                field(treatment_measurements, "cost", "treatment.measurements")?,
                "treatment.cost",
            )?;
            if string(control_cost, "status", "control.cost")? != "observed"
                || string(treatment_cost, "status", "treatment.cost")? != "observed"
                || string(control_cost, "currency", "control.cost")?
                    != string(treatment_cost, "currency", "treatment.cost")?
                || unsigned(
                    treatment_cost,
                    "amount_micros",
                    "treatment.cost",
                    i64::MAX as u64,
                )? >= unsigned(
                    control_cost,
                    "amount_micros",
                    "control.cost",
                    i64::MAX as u64,
                )?
            {
                return Err(
                    "supported cost_reduction lacks observed lower same-currency cost".to_string(),
                );
            }
        }
        "token_reduction" => {
            if string(control_measurements, "status", "control.measurements")? != "observed"
                || string(treatment_measurements, "status", "treatment.measurements")? != "observed"
                || unsigned(
                    treatment_measurements,
                    "input_tokens",
                    "treatment.measurements",
                    i64::MAX as u64,
                )? >= unsigned(
                    control_measurements,
                    "input_tokens",
                    "control.measurements",
                    i64::MAX as u64,
                )?
            {
                return Err(
                    "supported token_reduction lacks observed lower input tokens".to_string(),
                );
            }
        }
        "quality_preserved" if string(quality, "status", "quality")? != "preserved" => {
            return Err("supported quality_preserved requires preserved quality".to_string());
        }
        "latency_change" => {
            if string(control_measurements, "status", "control.measurements")? != "observed"
                || string(treatment_measurements, "status", "treatment.measurements")? != "observed"
                || unsigned(
                    control_measurements,
                    "latency_ms",
                    "control.measurements",
                    i64::MAX as u64,
                )? == unsigned(
                    treatment_measurements,
                    "latency_ms",
                    "treatment.measurements",
                    i64::MAX as u64,
                )?
            {
                return Err("supported latency_change lacks observed changed latency".to_string());
            }
        }
        "replayability"
            if !matches!(
                string(replay, "status", "replay")?,
                "replayable" | "partial"
            ) =>
        {
            return Err("supported replayability requires replay evidence".to_string());
        }
        "other" => {
            return Err(
                "supported other claims require a future explicit verifier rule".to_string(),
            )
        }
        _ => {}
    }
    Ok(())
}

fn parse_trust_store(raw: &[u8]) -> Result<TrustStore, String> {
    let value = parse_canonical_json(raw, "trust store")?;
    let store: TrustStore =
        serde_json::from_value(value).map_err(|error| format!("invalid trust store: {error}"))?;
    if store.schema_version != TRUST_SCHEMA
        || store.trust_revision == 0
        || !is_rfc3339_utc_timestamp(&store.evaluated_at)
        || store.trusted_signers.is_empty()
        || store.receipt_chain_heads.is_empty()
    {
        return Err(
            "trust store schema, revision, evaluation time, or signers is invalid".to_string(),
        );
    }
    let mut identities = BTreeSet::new();
    let mut receipt_aliases = BTreeSet::new();
    for signer in &store.trusted_signers {
        validate_trusted_signer(signer, &store.evaluated_at, store.trust_revision)?;
        if !identities.insert((signer.trusted_signer_ref.clone(), signer.key_id.clone())) {
            return Err("trust store contains duplicate signer identity".to_string());
        }
        if signer
            .receipt_key_ids
            .iter()
            .any(|alias| !receipt_aliases.insert(alias))
        {
            return Err("trust store contains duplicate receipt key alias".to_string());
        }
    }
    let mut chains = BTreeSet::new();
    for head in &store.receipt_chain_heads {
        if head.chain_id.is_empty()
            || head.chain_id.len() > 256
            || head.chain_id.chars().any(char::is_control)
            || head.sequence_number == 0
            || head.sequence_number > 9_007_199_254_740_991
            || !is_digest(&head.receipt_id)
            || !chains.insert(head.chain_id.clone())
        {
            return Err("trust store receipt chain head is malformed or duplicate".to_string());
        }
    }
    Ok(store)
}

fn validate_trusted_signer(
    signer: &TrustedSigner,
    evaluated_at: &str,
    trust_revision: u64,
) -> Result<(), String> {
    if !is_signer_ref(&signer.trusted_signer_ref)
        || !is_content_id(&signer.key_id)
        || !is_hex(&signer.public_key, 64)
        || signer.revision == 0
        || signer.revision > trust_revision
        || !is_rfc3339_utc_timestamp(&signer.admitted_at)
        || signer
            .expires_at
            .as_deref()
            .is_some_and(|value| !is_rfc3339_utc_timestamp(value))
        || signer
            .revoked_at
            .as_deref()
            .is_some_and(|value| !is_rfc3339_utc_timestamp(value))
        || signer.receipt_key_ids.is_empty()
        || signer
            .receipt_key_ids
            .iter()
            .any(|key_id| !is_receipt_key_id(key_id))
        || signer.allowed_trust_bases.is_empty()
        || signer
            .allowed_trust_bases
            .iter()
            .any(|basis| !matches!(basis.as_str(), "customer_configured" | "out_of_band"))
    {
        return Err("trust store signer is malformed or permits an unsupported basis".to_string());
    }
    if signer
        .expires_at
        .as_deref()
        .is_some_and(|expires| expires <= signer.admitted_at.as_str())
        || signer.revoked_at.as_deref().is_some_and(|revoked| {
            revoked <= signer.admitted_at.as_str() || revoked <= evaluated_at
        })
    {
        return Err("trust store signer is expired or revoked at evaluation time".to_string());
    }
    let key = hex_decode(&signer.public_key)
        .ok_or_else(|| "trust store public key is invalid hex".to_string())?;
    let expected_id = format!("id:sha256:{}", sha256_hex(&key));
    if signer.key_id != expected_id || signer.trusted_signer_ref != format!("signer:{expected_id}")
    {
        return Err("trust store signer key identity does not derive from public key".to_string());
    }
    Ok(())
}

fn is_receipt_key_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 128
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bytes.all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
        && !value.starts_with("base64:")
        && !value.starts_with("hex:")
}

fn trusted_key(document: &Value, store: &TrustStore) -> Result<VerifyingKey, String> {
    let root = object(document, "bundle")?;
    let signing = object(field(root, "signing", "bundle")?, "signing")?;
    let signer_ref = string(signing, "trusted_signer_ref", "signing")?;
    let key_id = string(signing, "key_id", "signing")?;
    let basis = string(signing, "trust_basis", "signing")?;
    let created_at = string(root, "created_at", "bundle")?;
    if basis == "local_identity" || signer_ref != format!("signer:{key_id}") {
        return Err("bundle signer must use an externally trusted key identity".to_string());
    }
    let matching: Vec<&TrustedSigner> = store
        .trusted_signers
        .iter()
        .filter(|signer| signer.trusted_signer_ref == signer_ref && signer.key_id == key_id)
        .collect();
    if matching.len() != 1
        || !matching[0]
            .allowed_trust_bases
            .iter()
            .any(|allowed| allowed == basis)
        || created_at < matching[0].admitted_at.as_str()
        || matching[0]
            .expires_at
            .as_deref()
            .is_some_and(|expires| created_at >= expires)
        || store.evaluated_at.as_str() < created_at
    {
        return Err(
            "no unique currently valid trust-store signer authorizes this bundle".to_string(),
        );
    }
    verifying_key(matching[0])
}

pub(crate) fn trusted_receipt_key(
    key_id: &str,
    issued_at: &str,
    store: &TrustStore,
) -> Result<VerifyingKey, String> {
    let matching: Vec<&TrustedSigner> = store
        .trusted_signers
        .iter()
        .filter(|signer| {
            signer
                .receipt_key_ids
                .iter()
                .any(|candidate| candidate == key_id)
        })
        .collect();
    if matching.len() != 1
        || issued_at < matching[0].admitted_at.as_str()
        || matching[0]
            .expires_at
            .as_deref()
            .is_some_and(|expires| issued_at >= expires)
        || store.evaluated_at.as_str() < issued_at
    {
        return Err("receipt signer is not uniquely trusted for issued_at".to_string());
    }
    verifying_key(matching[0])
}

fn verifying_key(signer: &TrustedSigner) -> Result<VerifyingKey, String> {
    let bytes = hex_decode(&signer.public_key)
        .ok_or_else(|| "trusted public key is invalid".to_string())?;
    VerifyingKey::from_bytes(
        &bytes
            .try_into()
            .map_err(|_| "trusted public key length is invalid".to_string())?,
    )
    .map_err(|_| "trusted public key is not an Ed25519 key".to_string())
}

fn verify_signature(document: &Value, unsigned: &[u8], key: &VerifyingKey) -> Result<(), String> {
    let root = object(document, "bundle")?;
    let signing = object(field(root, "signing", "bundle")?, "signing")?;
    let encoded = string(signing, "signature", "signing")?;
    let signature = STANDARD
        .decode(encoded)
        .map_err(|_| "signature is not standard Base64".to_string())?;
    if STANDARD.encode(&signature) != encoded {
        return Err("signature is not canonical standard Base64".to_string());
    }
    let signature = Signature::from_slice(&signature)
        .map_err(|_| "signature is not an Ed25519 signature".to_string())?;
    key.verify(unsigned, &signature)
        .map_err(|_| "Ed25519 signature does not verify under external trusted key".to_string())
}
