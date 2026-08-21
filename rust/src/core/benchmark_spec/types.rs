use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkSpecV1 {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub suite: BenchmarkSuite,
    pub configuration: BenchmarkConfiguration,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkSuite {
    pub kind: BenchmarkKind,
    pub tasks: Vec<BenchmarkTask>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum BenchmarkKind {
    Compression,
    QualityReplay,
    TaskScore,
    ABStudy,
    Comparison,
    Pipeline,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: TaskKind,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum TaskKind {
    Explore,
    LocateRegression,
    FixBug,
    RunTests,
    ExplainArchitecture,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkConfiguration {
    pub profile_hash: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub runtime_version: String,
    pub repeats: u32,
    pub quality_floor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkResult {
    pub spec_id: String,
    pub spec_version: String,
    pub profile_hash: String,
    pub agent: String,
    pub model: String,
    pub runtime_version: String,
    pub outcomes: Vec<BenchmarkOutcome>,
    pub summary: BenchmarkSummary,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkOutcome {
    pub task_id: String,
    pub passed: bool,
    pub cost_usd: f64,
    pub quality_score: f64,
    pub latency_ms: u64,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BenchmarkSummary {
    pub total_tasks: u32,
    pub passed_tasks: u32,
    pub pass_rate: f64,
    pub total_cost_usd: f64,
    pub cost_per_task_usd: f64,
    pub mean_quality: f64,
    pub mean_latency_ms: f64,
    pub quality_floor_met: bool,
}

impl BenchmarkSummary {
    pub(crate) fn from_outcomes(outcomes: &[BenchmarkOutcome], quality_floor: f64) -> Self {
        let total_tasks = outcomes.len() as u32;
        let passed_tasks = outcomes.iter().filter(|o| o.passed).count() as u32;
        let pass_rate = if total_tasks > 0 {
            f64::from(passed_tasks) / f64::from(total_tasks)
        } else {
            0.0
        };
        let total_cost_usd: f64 = outcomes.iter().map(|o| o.cost_usd).sum();
        let cost_per_task_usd = if total_tasks > 0 {
            total_cost_usd / f64::from(total_tasks)
        } else {
            0.0
        };
        let mean_quality = if total_tasks > 0 {
            outcomes.iter().map(|o| o.quality_score).sum::<f64>() / f64::from(total_tasks)
        } else {
            0.0
        };
        let mean_latency_ms = if total_tasks > 0 {
            outcomes.iter().map(|o| o.latency_ms as f64).sum::<f64>() / f64::from(total_tasks)
        } else {
            0.0
        };
        Self {
            total_tasks,
            passed_tasks,
            pass_rate,
            total_cost_usd,
            cost_per_task_usd,
            mean_quality,
            mean_latency_ms,
            quality_floor_met: mean_quality >= quality_floor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_outcome(passed: bool, cost: f64, quality: f64) -> BenchmarkOutcome {
        BenchmarkOutcome {
            task_id: "t1".into(),
            passed,
            cost_usd: cost,
            quality_score: quality,
            latency_ms: 100,
            tokens_input: 1000,
            tokens_output: 200,
            error: None,
        }
    }

    #[test]
    fn spec_roundtrip() {
        let spec = BenchmarkSpecV1 {
            id: "bench-001".into(),
            version: "1.0.0".into(),
            name: "LeanBench Coding".into(),
            description: "Standard benchmark".into(),
            suite: BenchmarkSuite {
                kind: BenchmarkKind::TaskScore,
                tasks: vec![BenchmarkTask {
                    id: "t1".into(),
                    name: "Explore".into(),
                    description: "Navigate codebase".into(),
                    kind: TaskKind::Explore,
                    timeout_ms: Some(60_000),
                }],
            },
            configuration: BenchmarkConfiguration {
                profile_hash: Some("abc".into()),
                agent: Some("codex".into()),
                model: Some("gpt-4".into()),
                runtime_version: "1.9.0".into(),
                repeats: 3,
                quality_floor: 0.95,
            },
            created_at: "2026-08-21T09:00:00Z".into(),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let restored: BenchmarkSpecV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, spec.id);
        assert_eq!(restored.suite.kind, BenchmarkKind::TaskScore);
    }

    #[test]
    fn summary_from_outcomes() {
        let outcomes = vec![
            sample_outcome(true, 0.20, 0.97),
            sample_outcome(false, 0.18, 0.60),
        ];
        let summary = BenchmarkSummary::from_outcomes(&outcomes, 0.95);
        assert_eq!(summary.total_tasks, 2);
        assert_eq!(summary.passed_tasks, 1);
        assert!(!summary.quality_floor_met);
    }

    #[test]
    fn kind_roundtrip() {
        for kind in [
            BenchmarkKind::Compression,
            BenchmarkKind::QualityReplay,
            BenchmarkKind::TaskScore,
            BenchmarkKind::Custom,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let restored: BenchmarkKind = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, kind);
        }
    }
}
