//! Explicit, evidence-qualified manual profile selection.
//!
//! The record is local and portable. It makes no learned ranking or automatic
//! promotion decision: an operator chooses when to write and apply it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::canonical::canonical_serialize;
use crate::core::config;
use crate::core::profiles;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionEvidenceArmV1 {
    pub candidate_id: String,
    pub profile_name: String,
    pub profile_hash: String,
    pub spec_id: String,
    pub spec_version: String,
    pub spec_digest: String,
    pub result_digest: String,
    pub receipt_refs: Vec<String>,
}

/// Immutable selection constraints captured with the candidate evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionPolicyV1 {
    pub quality_floor: f64,
    pub requires_evaluated_quality: bool,
    pub requires_complete_receipts: bool,
}

/// Deterministic, human-readable basis for the explicitly selected candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionRationaleV1 {
    pub kind: String,
    pub selected_cost_per_task: f64,
    pub selected_mean_quality: f64,
    pub selected_mean_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManualSelectionRecordV1 {
    pub schema_version: u32,
    pub selection_id: String,
    pub previous_profile: String,
    pub selected_profile: String,
    pub selected_candidate_id: String,
    pub evidence_bundle_sha256: String,
    pub policy: SelectionPolicyV1,
    pub rationale: SelectionRationaleV1,
    pub evidence_arms: Vec<SelectionEvidenceArmV1>,
}

impl ManualSelectionRecordV1 {
    pub(crate) fn create(
        previous_profile: String,
        selected_profile: String,
        selected_candidate_id: String,
        evidence_bundle_sha256: String,
        policy: SelectionPolicyV1,
        rationale: SelectionRationaleV1,
        mut evidence_arms: Vec<SelectionEvidenceArmV1>,
    ) -> Result<Self, String> {
        evidence_arms.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
        for arm in &mut evidence_arms {
            arm.receipt_refs.sort();
            arm.receipt_refs.dedup();
        }
        let mut record = Self {
            schema_version: SCHEMA_VERSION,
            selection_id: String::new(),
            previous_profile,
            selected_profile,
            selected_candidate_id,
            evidence_bundle_sha256,
            policy,
            rationale,
            evidence_arms,
        };
        record.validate()?;
        record.selection_id = selection_id(&record);
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported manual selection schema {}",
                self.schema_version
            ));
        }
        for (label, value) in [
            ("previous_profile", &self.previous_profile),
            ("selected_profile", &self.selected_profile),
            ("selected_candidate_id", &self.selected_candidate_id),
            ("evidence_bundle_sha256", &self.evidence_bundle_sha256),
        ] {
            if value.trim().is_empty() {
                return Err(format!("manual selection {label} is required"));
            }
        }
        if !is_hex_digest(&self.evidence_bundle_sha256) {
            return Err("manual selection evidence_bundle_sha256 must be a SHA-256 digest".into());
        }
        if !self.policy.quality_floor.is_finite()
            || !(0.0..=1.0).contains(&self.policy.quality_floor)
            || !self.policy.requires_evaluated_quality
            || !self.policy.requires_complete_receipts
        {
            return Err(
                "manual selection policy must require evaluated receipt-linked quality".into(),
            );
        }
        if !matches!(
            self.rationale.kind.as_str(),
            "lowest-cost-above-floor" | "only-candidate"
        ) || !self.rationale.selected_cost_per_task.is_finite()
            || self.rationale.selected_cost_per_task < 0.0
            || !self.rationale.selected_mean_quality.is_finite()
            || !(0.0..=1.0).contains(&self.rationale.selected_mean_quality)
            || !self.rationale.selected_mean_latency_ms.is_finite()
            || self.rationale.selected_mean_latency_ms < 0.0
        {
            return Err("manual selection rationale is invalid".into());
        }
        if self.evidence_arms.len() < 2 {
            return Err("manual selection requires at least two evidence arms".into());
        }

        let mut candidate_ids = std::collections::BTreeSet::new();
        let mut selected = false;
        for arm in &self.evidence_arms {
            for (label, value) in [
                ("candidate_id", &arm.candidate_id),
                ("profile_name", &arm.profile_name),
                ("profile_hash", &arm.profile_hash),
                ("spec_id", &arm.spec_id),
                ("spec_version", &arm.spec_version),
                ("spec_digest", &arm.spec_digest),
                ("result_digest", &arm.result_digest),
            ] {
                if value.trim().is_empty() {
                    return Err(format!("manual selection evidence {label} is required"));
                }
            }
            if !candidate_ids.insert(&arm.candidate_id) {
                return Err(format!(
                    "manual selection evidence candidate '{}' is duplicated",
                    arm.candidate_id
                ));
            }
            if !is_blake3_digest(&arm.spec_digest) || !is_blake3_digest(&arm.result_digest) {
                return Err("manual selection evidence digests must be BLAKE3 digests".into());
            }
            if arm.receipt_refs.is_empty()
                || !arm.receipt_refs.windows(2).all(|pair| pair[0] < pair[1])
                || arm
                    .receipt_refs
                    .iter()
                    .any(|reference| reference.strip_prefix("receipt:").is_none_or(str::is_empty))
            {
                return Err(
                    "manual selection evidence requires canonical receipt references".into(),
                );
            }
            if arm.candidate_id == self.selected_candidate_id {
                selected = arm.profile_name == self.selected_profile;
            }
        }
        if !selected {
            return Err("selected profile must match an evidence-qualified candidate".into());
        }
        if !self.selection_id.is_empty() && self.selection_id != selection_id(self) {
            return Err("manual selection record digest does not match its contents".into());
        }
        Ok(())
    }
}

pub(crate) fn write_record(path: &Path, record: &ManualSelectionRecordV1) -> Result<(), String> {
    record.validate()?;
    let bytes = canonical_serialize(record);
    crate::core::atomic_fs::write_bytes_with_fallback(path, &bytes, None)
}

pub(crate) fn read_record(path: &Path) -> Result<ManualSelectionRecordV1, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read selection record: {error}"))?;
    let record: ManualSelectionRecordV1 = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse selection record: {error}"))?;
    record.validate()?;
    Ok(record)
}

/// Apply an explicitly written selection to the global profile config.
pub(crate) fn apply_record(record: &ManualSelectionRecordV1) -> Result<(), String> {
    record.validate()?;
    reject_environment_override()?;
    let current_profile =
        config::setter::current_value("profile").unwrap_or_else(|| "coder".to_string());
    if current_profile != record.previous_profile {
        return Err(
            "refusing apply: configured profile no longer matches this selection record's previous profile"
                .into(),
        );
    }
    if profiles::load_profile(&record.selected_profile).is_none() {
        return Err(format!(
            "selected profile '{}' no longer exists",
            record.selected_profile
        ));
    }
    config::setter::set_by_key("profile", &record.selected_profile)
        .map(|_| ())
        .map_err(|error| format!("apply selected profile: {error}"))
}

/// Restore the profile that was active when this record was created.
pub(crate) fn rollback_record(record: &ManualSelectionRecordV1) -> Result<(), String> {
    record.validate()?;
    reject_environment_override()?;
    if config::setter::current_value("profile").as_deref() != Some(&record.selected_profile) {
        return Err(
            "refusing rollback: configured profile no longer matches this selection record".into(),
        );
    }
    if profiles::load_profile(&record.previous_profile).is_none() {
        return Err(format!(
            "previous profile '{}' no longer exists",
            record.previous_profile
        ));
    }
    config::setter::set_by_key("profile", &record.previous_profile)
        .map(|_| ())
        .map_err(|error| format!("restore previous profile: {error}"))
}

fn selection_id(record: &ManualSelectionRecordV1) -> String {
    let mut unsigned = record.clone();
    unsigned.selection_id.clear();
    format!(
        "selection-{}",
        blake3::hash(&canonical_serialize(&unsigned)).to_hex()
    )
}

fn reject_environment_override() -> Result<(), String> {
    if std::env::var("LEAN_CTX_PROFILE")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Err("LEAN_CTX_PROFILE overrides config; unset it before apply or rollback".into());
    }
    Ok(())
}

fn is_hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_blake3_digest(value: &str) -> bool {
    value.strip_prefix("blake3:").is_some_and(is_hex_digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(candidate_id: &str, profile_name: &str) -> SelectionEvidenceArmV1 {
        SelectionEvidenceArmV1 {
            candidate_id: candidate_id.into(),
            profile_name: profile_name.into(),
            profile_hash: "profile-hash".into(),
            spec_id: "workload".into(),
            spec_version: "1.0.0".into(),
            spec_digest: format!("blake3:{}", "a".repeat(64)),
            result_digest: format!("blake3:{}", "b".repeat(64)),
            receipt_refs: vec!["receipt:receipt-proof".into()],
        }
    }

    fn policy() -> SelectionPolicyV1 {
        SelectionPolicyV1 {
            quality_floor: 0.95,
            requires_evaluated_quality: true,
            requires_complete_receipts: true,
        }
    }

    fn rationale() -> SelectionRationaleV1 {
        SelectionRationaleV1 {
            kind: "lowest-cost-above-floor".into(),
            selected_cost_per_task: 0.1,
            selected_mean_quality: 0.99,
            selected_mean_latency_ms: 10.0,
        }
    }

    #[test]
    fn canonical_record_is_stable_across_evidence_order() {
        let first = ManualSelectionRecordV1::create(
            "coder".into(),
            "exploration".into(),
            "profile-exploration".into(),
            "c".repeat(64),
            policy(),
            rationale(),
            vec![
                arm("profile-exploration", "exploration"),
                arm("profile-coder", "coder"),
            ],
        )
        .unwrap();
        let second = ManualSelectionRecordV1::create(
            "coder".into(),
            "exploration".into(),
            "profile-exploration".into(),
            "c".repeat(64),
            policy(),
            rationale(),
            vec![
                arm("profile-coder", "coder"),
                arm("profile-exploration", "exploration"),
            ],
        )
        .unwrap();

        assert_eq!(first.selection_id, second.selection_id);
        assert_eq!(canonical_serialize(&first), canonical_serialize(&second));
    }

    #[test]
    fn record_rejects_unlinked_or_mismatched_selection_evidence() {
        let mut invalid = arm("profile-exploration", "exploration");
        invalid.receipt_refs.clear();
        assert!(
            ManualSelectionRecordV1::create(
                "coder".into(),
                "exploration".into(),
                "profile-exploration".into(),
                "c".repeat(64),
                policy(),
                rationale(),
                vec![invalid, arm("profile-coder", "coder")],
            )
            .is_err()
        );
    }

    #[test]
    fn record_round_trip_is_atomic_and_portable() {
        let record = ManualSelectionRecordV1::create(
            "coder".into(),
            "exploration".into(),
            "profile-exploration".into(),
            "c".repeat(64),
            policy(),
            rationale(),
            vec![
                arm("profile-exploration", "exploration"),
                arm("profile-coder", "coder"),
            ],
        )
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("selection.json");

        write_record(&path, &record).unwrap();
        let restored = read_record(&path).unwrap();

        assert_eq!(restored.selection_id, record.selection_id);
        assert_eq!(canonical_serialize(&restored), canonical_serialize(&record));
    }

    #[test]
    fn apply_and_rollback_preserve_the_previous_profile_in_an_isolated_config() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        config::setter::set_by_key("profile", "coder").unwrap();
        let record = ManualSelectionRecordV1::create(
            "coder".into(),
            "exploration".into(),
            "profile-exploration".into(),
            "c".repeat(64),
            policy(),
            rationale(),
            vec![
                arm("profile-exploration", "exploration"),
                arm("profile-coder", "coder"),
            ],
        )
        .unwrap();

        apply_record(&record).unwrap();
        assert_eq!(
            config::setter::current_value("profile").as_deref(),
            Some("exploration")
        );
        rollback_record(&record).unwrap();
        assert_eq!(
            config::setter::current_value("profile").as_deref(),
            Some("coder")
        );
    }

    #[test]
    fn apply_rejects_a_record_when_the_current_profile_is_stale() {
        let _data_dir = crate::core::data_dir::isolated_data_dir();
        config::setter::set_by_key("profile", "exploration").unwrap();
        let record = ManualSelectionRecordV1::create(
            "coder".into(),
            "exploration".into(),
            "profile-exploration".into(),
            "c".repeat(64),
            policy(),
            rationale(),
            vec![
                arm("profile-exploration", "exploration"),
                arm("profile-coder", "coder"),
            ],
        )
        .unwrap();

        assert!(
            apply_record(&record)
                .unwrap_err()
                .contains("previous profile")
        );
        assert_eq!(
            config::setter::current_value("profile").as_deref(),
            Some("exploration")
        );
    }
}
