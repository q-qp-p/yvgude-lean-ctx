use super::config::CalibrationConfig;
use super::pareto::CalibratedResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Recommendation {
    pub candidate_id: String,
    pub label: String,
    pub cost_per_task: f64,
    pub mean_quality: f64,
    pub mean_latency_ms: f64,
    pub reason: RecommendationReason,
    pub vs_baseline: Option<BaselineComparison>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum RecommendationReason {
    LowestCostAboveFloor,
    ClosestToFloor,
    OnlyCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BaselineComparison {
    pub cost_delta_pct: f64,
    pub quality_delta: f64,
    pub latency_delta_pct: f64,
}

pub(crate) fn recommend(
    all_results: &[CalibratedResult],
    frontier: &[CalibratedResult],
    config: &CalibrationConfig,
) -> Option<Recommendation> {
    let baseline = all_results.first()?;
    if let Some(best) = frontier.iter().min_by(|a, b| {
        a.cost_per_task
            .partial_cmp(&b.cost_per_task)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        let comparison = BaselineComparison {
            cost_delta_pct: if baseline.cost_per_task > 0.0 {
                (best.cost_per_task - baseline.cost_per_task) / baseline.cost_per_task * 100.0
            } else {
                0.0
            },
            quality_delta: best.mean_quality - baseline.mean_quality,
            latency_delta_pct: if baseline.mean_latency_ms > 0.0 {
                (best.mean_latency_ms - baseline.mean_latency_ms) / baseline.mean_latency_ms * 100.0
            } else {
                0.0
            },
        };
        let reason = if frontier.len() == 1 {
            RecommendationReason::OnlyCandidate
        } else {
            RecommendationReason::LowestCostAboveFloor
        };
        return Some(Recommendation {
            candidate_id: best.candidate.id.clone(),
            label: best.candidate.label.clone(),
            cost_per_task: best.cost_per_task,
            mean_quality: best.mean_quality,
            mean_latency_ms: best.mean_latency_ms,
            reason,
            vs_baseline: Some(comparison),
        });
    }
    all_results
        .iter()
        .min_by(|a, b| {
            let da = (a.mean_quality - config.quality_floor).abs();
            let db = (b.mean_quality - config.quality_floor).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|closest| Recommendation {
            candidate_id: closest.candidate.id.clone(),
            label: closest.candidate.label.clone(),
            cost_per_task: closest.cost_per_task,
            mean_quality: closest.mean_quality,
            mean_latency_ms: closest.mean_latency_ms,
            reason: RecommendationReason::ClosestToFloor,
            vs_baseline: None,
        })
}

#[cfg(test)]
mod tests {
    use super::super::candidate::CandidateProfile;
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
    fn picks_cheapest() {
        let all = vec![result("baseline", 1.00, 0.96), result("cheap", 0.38, 0.96)];
        let frontier = vec![result("cheap", 0.38, 0.96)];
        let rec = recommend(&all, &frontier, &CalibrationConfig::default()).unwrap();
        assert_eq!(rec.candidate_id, "cheap");
    }

    #[test]
    fn fallback_closest() {
        let all = vec![result("baseline", 1.00, 0.90), result("close", 0.50, 0.93)];
        let rec = recommend(&all, &[], &CalibrationConfig::default()).unwrap();
        assert_eq!(rec.candidate_id, "close");
    }

    #[test]
    fn empty_none() {
        assert!(recommend(&[], &[], &CalibrationConfig::default()).is_none());
    }
}
