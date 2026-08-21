use super::candidate::CandidateProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CalibratedResult {
    pub candidate: CandidateProfile,
    pub cost_per_task: f64,
    pub mean_quality: f64,
    pub mean_latency_ms: f64,
    pub pass_rate: f64,
    pub quality_floor_met: bool,
}

pub(crate) fn compute_pareto_frontier(
    results: &[CalibratedResult],
    quality_floor: f64,
) -> Vec<CalibratedResult> {
    let mut eligible: Vec<_> = results
        .iter()
        .filter(|r| r.mean_quality >= quality_floor)
        .cloned()
        .collect();
    eligible.sort_by(|a, b| {
        a.cost_per_task
            .partial_cmp(&b.cost_per_task)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut frontier = Vec::new();
    let mut best_quality = f64::NEG_INFINITY;
    for r in &eligible {
        if r.mean_quality > best_quality {
            best_quality = r.mean_quality;
            frontier.push(r.clone());
        }
    }
    frontier
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(id: &str, cost: f64, quality: f64) -> CalibratedResult {
        CalibratedResult {
            candidate: CandidateProfile {
                id: id.into(),
                label: id.into(),
                budget_tokens: 32_000,
                compression: "balanced".into(),
                reuse_threshold: 0.85,
                capability_variant: "leanctx".into(),
            },
            cost_per_task: cost,
            mean_quality: quality,
            mean_latency_ms: 100.0,
            pass_rate: 1.0,
            quality_floor_met: quality >= 0.95,
        }
    }

    #[test]
    fn pareto_filters_below_floor() {
        let results = vec![
            result("a", 0.50, 0.98),
            result("b", 0.30, 0.90),
            result("c", 0.40, 0.96),
        ];
        let frontier = compute_pareto_frontier(&results, 0.95);
        assert!(frontier.iter().all(|r| r.mean_quality >= 0.95));
    }

    #[test]
    fn empty_input() {
        assert!(compute_pareto_frontier(&[], 0.95).is_empty());
    }

    #[test]
    fn all_below_floor() {
        let results = vec![result("a", 0.10, 0.80), result("b", 0.20, 0.85)];
        assert!(compute_pareto_frontier(&results, 0.95).is_empty());
    }
}
