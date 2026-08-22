use super::config::CalibrationConfig;
use super::pareto::CalibratedResult;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

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
    _config: &CalibrationConfig,
) -> Option<Recommendation> {
    let baseline = all_results.first()?;
    if let Some(best) = frontier
        .iter()
        .filter(|result| result.receipt_evidence_complete)
        .min_by(|a, b| compare_by_preference(a, b))
    {
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
    None
}

fn compare_by_preference(a: &CalibratedResult, b: &CalibratedResult) -> Ordering {
    a.cost_per_task
        .total_cmp(&b.cost_per_task)
        .then_with(|| b.mean_quality.total_cmp(&a.mean_quality))
        .then_with(|| a.mean_latency_ms.total_cmp(&b.mean_latency_ms))
        .then_with(|| a.candidate.id.cmp(&b.candidate.id))
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
            quality_evaluated: true,
            receipt_evidence_complete: true,
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
    fn all_below_floor_returns_no_recommendation() {
        let all = vec![result("baseline", 1.00, 0.90), result("close", 0.50, 0.93)];
        assert!(recommend(&all, &[], &CalibrationConfig::default()).is_none());
    }

    #[test]
    fn empty_none() {
        assert!(recommend(&[], &[], &CalibrationConfig::default()).is_none());
    }

    #[test]
    fn incomplete_receipt_evidence_cannot_produce_recommendation() {
        let mut incomplete = result("incomplete", 0.10, 0.99);
        incomplete.receipt_evidence_complete = false;

        assert!(
            recommend(
                &[incomplete.clone()],
                &[incomplete],
                &CalibrationConfig::default()
            )
            .is_none()
        );
    }

    #[test]
    fn one_frontier_point_is_marked_as_the_only_candidate() {
        let all = vec![result("baseline", 1.00, 0.90), result("only", 0.50, 0.96)];
        let frontier = vec![result("only", 0.50, 0.96)];
        let rec = recommend(&all, &frontier, &CalibrationConfig::default()).unwrap();
        assert_eq!(rec.candidate_id, "only");
        assert!(matches!(rec.reason, RecommendationReason::OnlyCandidate));
    }

    #[test]
    fn ties_use_quality_latency_then_candidate_id() {
        let mut faster = result("faster", 0.50, 0.96);
        faster.mean_latency_ms = 90.0;
        let mut slower = result("slower", 0.50, 0.96);
        slower.mean_latency_ms = 100.0;
        let all = vec![slower.clone(), faster.clone()];
        let rec = recommend(&all, &[slower, faster], &CalibrationConfig::default()).unwrap();
        assert_eq!(rec.candidate_id, "faster");

        let all = vec![result("tie-b", 0.50, 0.96), result("tie-a", 0.50, 0.96)];
        let rec = recommend(&all, &all, &CalibrationConfig::default()).unwrap();
        assert_eq!(rec.candidate_id, "tie-a");
    }
}
