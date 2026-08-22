pub(crate) mod candidate;
pub(crate) mod config;
pub(crate) mod pareto;
pub(crate) mod recommendation;
pub(crate) mod report;
pub(crate) mod selection;

pub(crate) use config::CalibrationConfig;
use pareto::CalibratedResult;

pub(crate) struct CalibrationReport {
    pub results: Vec<CalibratedResult>,
    pub frontier: Vec<CalibratedResult>,
    pub recommendation: Option<recommendation::Recommendation>,
    pub report_text: String,
}

pub(crate) fn calibrate(
    results: Vec<CalibratedResult>,
    config: &CalibrationConfig,
) -> CalibrationReport {
    let frontier = pareto::compute_pareto_frontier(&results, config.quality_floor);
    let rec = recommendation::recommend(&results, &frontier, config);
    let report_text = report::format_calibration_report(&results, rec.as_ref());
    CalibrationReport {
        results,
        frontier,
        recommendation: rec,
        report_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(id: &str, cost: f64, quality: f64) -> CalibratedResult {
        CalibratedResult {
            candidate: candidate::CandidateProfile {
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
    fn calibrate_end_to_end() {
        let results = vec![
            result("baseline", 1.00, 0.964),
            result("default", 0.49, 0.966),
            result("aggressive", 0.31, 0.912),
            result("leanctx-rtk", 0.38, 0.968),
            result("leanctx-graph", 0.42, 0.981),
        ];
        let report = calibrate(results, &CalibrationConfig::default());
        assert!(!report.frontier.is_empty());
        assert!(report.recommendation.is_some());
        assert!(report.report_text.contains("LINKED"));
    }

    #[test]
    fn calibrate_all_fail_quality() {
        let results = vec![result("bad-a", 0.10, 0.80), result("bad-b", 0.20, 0.85)];
        let report = calibrate(results, &CalibrationConfig::default());
        assert!(report.frontier.is_empty());
        assert!(report.recommendation.is_none());
        assert!(report.report_text.contains("No recommendation"));
    }

    #[test]
    fn calibrate_single_candidate() {
        let report = calibrate(
            vec![result("only", 0.40, 0.96)],
            &CalibrationConfig::default(),
        );
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.frontier.len(), 1);
        let recommendation = report.recommendation.expect("single candidate is eligible");
        assert_eq!(recommendation.candidate_id, "only");
        assert!(matches!(
            recommendation.reason,
            recommendation::RecommendationReason::OnlyCandidate
        ));
    }

    #[test]
    fn calibrate_empty_results_has_no_recommendation() {
        let report = calibrate(Vec::new(), &CalibrationConfig::default());
        assert!(report.frontier.is_empty());
        assert!(report.recommendation.is_none());
        assert!(report.report_text.contains("No recommendation"));
    }
}
