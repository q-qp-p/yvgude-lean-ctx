use crate::core::benchmark_spec::types::BenchmarkResult;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReceiptV1 {
    pub receipt_id: String,
    pub spec_id: String,
    pub spec_version: String,
    pub profile_hash: String,
    pub agent: String,
    pub model: Option<String>,
    pub runtime_version: String,
    pub runner_info: ReceiptRunnerInfo,
    pub summary: ReceiptSummary,
    pub outcomes: Vec<ReceiptOutcome>,
    pub created_at: String,
    pub signature: Option<String>,
    pub verification: VerificationLevel,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReceiptRunnerInfo {
    pub os: String,
    pub arch: String,
    pub hostname_hash: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReceiptSummary {
    pub total_tasks: u32,
    pub passed_tasks: u32,
    pub pass_rate: f64,
    pub total_cost_usd: f64,
    pub cost_per_task_usd: f64,
    pub mean_quality: f64,
    pub mean_latency_ms: f64,
    pub quality_floor_met: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReceiptOutcome {
    pub task_id: String,
    pub passed: bool,
    pub cost_usd: f64,
    pub quality_score: f64,
    pub latency_ms: f64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) enum VerificationLevel {
    Unverified,
    CommunitySubmitted,
    Verified,
}

#[allow(dead_code)]
impl ReceiptV1 {
    pub(crate) fn from_benchmark_result(result: &BenchmarkResult) -> Self {
        let ts = unix_timestamp();
        Self {
            receipt_id: Self::receipt_id_from_parts(ts, &result.spec_id),
            spec_id: result.spec_id.clone(),
            spec_version: result.spec_version.clone(),
            profile_hash: result.profile_hash.clone(),
            agent: result.agent.clone(),
            model: Some(result.model.clone()),
            runtime_version: result.runtime_version.clone(),
            runner_info: ReceiptRunnerInfo {
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                hostname_hash: hash_hostname(),
            },
            summary: ReceiptSummary {
                total_tasks: result.summary.total_tasks,
                passed_tasks: result.summary.passed_tasks,
                pass_rate: result.summary.pass_rate,
                total_cost_usd: result.summary.total_cost_usd,
                cost_per_task_usd: result.summary.cost_per_task_usd,
                mean_quality: result.summary.mean_quality,
                mean_latency_ms: result.summary.mean_latency_ms,
                quality_floor_met: result.summary.quality_floor_met,
            },
            outcomes: result
                .outcomes
                .iter()
                .map(|o| ReceiptOutcome {
                    task_id: o.task_id.clone(),
                    passed: o.passed,
                    cost_usd: o.cost_usd,
                    quality_score: o.quality_score,
                    latency_ms: o.latency_ms as f64,
                })
                .collect(),
            created_at: ts.to_string(),
            signature: None,
            verification: VerificationLevel::Unverified,
        }
    }
    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
    pub(crate) fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
    pub(crate) fn receipt_id_from_parts(timestamp: u64, spec_id: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        spec_id.hash(&mut h);
        format!("rcpt-{timestamp}-{:08x}", h.finish() as u32)
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hash_hostname() -> String {
    use std::hash::{Hash, Hasher};
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".into());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hostname.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> ReceiptV1 {
        ReceiptV1 {
            receipt_id: "rcpt-100-abcd1234".into(),
            spec_id: "leanbench-coder".into(),
            spec_version: "1.0.0".into(),
            profile_hash: "abc123".into(),
            agent: "codex".into(),
            model: Some("gpt-4".into()),
            runtime_version: "3.9.19".into(),
            runner_info: ReceiptRunnerInfo {
                os: "macos".into(),
                arch: "aarch64".into(),
                hostname_hash: "deadbeef".into(),
            },
            summary: ReceiptSummary {
                total_tasks: 5,
                passed_tasks: 4,
                pass_rate: 0.8,
                total_cost_usd: 1.5,
                cost_per_task_usd: 0.3,
                mean_quality: 0.94,
                mean_latency_ms: 2000.0,
                quality_floor_met: false,
            },
            outcomes: vec![ReceiptOutcome {
                task_id: "t1".into(),
                passed: true,
                cost_usd: 0.3,
                quality_score: 0.95,
                latency_ms: 1500.0,
            }],
            created_at: "100".into(),
            signature: None,
            verification: VerificationLevel::Unverified,
        }
    }
    #[test]
    fn receipt_roundtrip_json() {
        let r = sample();
        let j = r.to_json();
        let p = ReceiptV1::from_json(&j).unwrap();
        assert_eq!(p.receipt_id, r.receipt_id);
    }
    #[test]
    fn receipt_id_format() {
        let id = ReceiptV1::receipt_id_from_parts(12345, "test-spec");
        assert!(id.starts_with("rcpt-12345-"));
    }
    #[test]
    fn verification_levels() {
        assert_ne!(VerificationLevel::Unverified, VerificationLevel::Verified);
    }
    #[test]
    fn from_parts_deterministic() {
        assert_eq!(
            ReceiptV1::receipt_id_from_parts(100, "a"),
            ReceiptV1::receipt_id_from_parts(100, "a")
        );
    }
    #[test]
    fn hash_hostname_stable() {
        assert_eq!(hash_hostname(), hash_hostname());
        assert_eq!(hash_hostname().len(), 16);
    }
}
