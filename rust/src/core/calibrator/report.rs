use super::pareto::CalibratedResult;
use super::recommendation::Recommendation;

pub(crate) fn format_calibration_report(
    results: &[CalibratedResult],
    recommendation: Option<&Recommendation>,
) -> String {
    let sep = "\u{2550}".repeat(76);
    let thin = "\u{2500}".repeat(76);
    let mut out = Vec::new();
    out.push(format!("  {sep}"));
    out.push("  CALIBRATION REPORT".into());
    out.push(format!("  {sep}"));
    out.push(format!(
        "  {:<30} {:>10} {:>10} {:>10} {:>10}",
        "PROFILE", "COST", "QUALITY", "LATENCY", "STATUS"
    ));
    out.push(format!("  {thin}"));
    for r in results {
        let status = match recommendation {
            Some(rec) if rec.candidate_id == r.candidate.id => "RECOMMENDED",
            _ if r.quality_floor_met => "PASS",
            _ => "FAILED",
        };
        out.push(format!(
            "  {:<30} {:>9.4}$ {:>9.1}% {:>8.0}ms {:>10}",
            &r.candidate.label[..r.candidate.label.len().min(30)],
            r.cost_per_task,
            r.mean_quality * 100.0,
            r.mean_latency_ms,
            status
        ));
    }
    out.push(format!("  {thin}"));
    if let Some(rec) = recommendation {
        if let Some(bl) = &rec.vs_baseline {
            out.push(format!(
                "  Recommended: {}  ({:+.1}% cost | {:+.3} quality | {:+.1}% latency)",
                rec.label, bl.cost_delta_pct, bl.quality_delta, bl.latency_delta_pct
            ));
        } else {
            out.push(format!(
                "  Recommended: {} (closest to quality floor)",
                rec.label
            ));
        }
    } else {
        out.push("  No recommendation \u{2014} no candidates available.".into());
    }
    out.push(format!("  {sep}"));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::candidate::CandidateProfile;
    use super::super::pareto::CalibratedResult;
    use super::super::recommendation::{BaselineComparison, Recommendation, RecommendationReason};
    use super::*;

    #[test]
    fn report_contains_recommended() {
        let results = vec![CalibratedResult {
            candidate: CandidateProfile {
                id: "opt".into(),
                label: "optimized".into(),
                budget_tokens: 32_000,
                compression: "balanced".into(),
                reuse_threshold: 0.85,
                capability_variant: "leanctx".into(),
            },
            cost_per_task: 0.40,
            mean_quality: 0.96,
            mean_latency_ms: 100.0,
            pass_rate: 1.0,
            quality_floor_met: true,
        }];
        let rec = Some(Recommendation {
            candidate_id: "opt".into(),
            label: "optimized".into(),
            cost_per_task: 0.40,
            mean_quality: 0.96,
            mean_latency_ms: 100.0,
            reason: RecommendationReason::LowestCostAboveFloor,
            vs_baseline: Some(BaselineComparison {
                cost_delta_pct: -60.0,
                quality_delta: 0.0,
                latency_delta_pct: 0.0,
            }),
        });
        let report = format_calibration_report(&results, rec.as_ref());
        assert!(report.contains("RECOMMENDED"));
    }
}
