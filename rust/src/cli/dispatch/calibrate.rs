use crate::core::benchmark_spec::profile_bridge;
use crate::core::calibrator::candidate::CandidateProfile;
use crate::core::calibrator::pareto::CalibratedResult;
use crate::core::calibrator::{self, CalibrationConfig};
use crate::core::profiles;

pub(crate) fn cmd_calibrate(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return 0;
    }

    let profile_name = args
        .first()
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("coder");

    let quality_floor = parse_flag_f64(args, "--quality-floor").unwrap_or(0.95);
    let max_candidates = parse_flag_usize(args, "--max-candidates").unwrap_or(12);

    eprintln!("lean-ctx calibrate");
    eprintln!("  Profile:        {profile_name}");
    eprintln!("  Quality floor:  {:.0}%", quality_floor * 100.0);
    eprintln!("  Max candidates: {max_candidates}");
    eprintln!();

    let Some(profile) = profiles::load_profile(profile_name) else {
        eprintln!("error: profile '{profile_name}' not found");
        eprintln!("  available: lean-ctx profile list");
        return 1;
    };

    let config = CalibrationConfig {
        quality_floor,
        max_candidates,
        budget_range: (
            profile.budget.max_context_tokens_effective() / 4,
            profile.budget.max_context_tokens_effective(),
        ),
        compression_levels: vec!["lossless".into(), "balanced".into(), "aggressive".into()],
        reuse_range: (0.70, 0.95),
        capability_variants: vec!["leanctx".into()],
    };

    let candidates = crate::core::calibrator::candidate::generate_candidates(&config);
    eprintln!("  Generated {} candidates", candidates.len());
    eprintln!();

    let results = simulate_benchmark_results(&candidates, &profile);

    let report = calibrator::calibrate(results, &config);

    println!("{}", report.report_text);

    if let Some(rec) = &report.recommendation {
        eprintln!();
        eprintln!("  Recommended profile: {}", rec.label);
        if let Some(bl) = &rec.vs_baseline {
            eprintln!("  Cost delta:    {:+.1}%", bl.cost_delta_pct);
            eprintln!("  Quality delta: {:+.4}", bl.quality_delta);
            eprintln!("  Latency delta: {:+.1}%", bl.latency_delta_pct);
        }
    }

    let suite = profile_bridge::default_coding_suite();
    match profile_bridge::create_spec(profile_name, suite) {
        Ok(spec) => {
            let json = serde_json::to_string_pretty(&spec).unwrap_or_default();
            eprintln!();
            eprintln!("  BenchmarkSpec written to stdout (pipe to file):");
            eprintln!("  lean-ctx calibrate {profile_name} --export-spec > spec.json");
            if args.iter().any(|a| a == "--export-spec") {
                println!("{json}");
            }
        }
        Err(e) => {
            eprintln!("  warning: could not create BenchmarkSpec: {e}");
        }
    }

    0
}

/// Simulated benchmark — real agent invocation will be wired in WS-6.9.
/// Uses profile settings to generate plausible cost/quality/latency curves.
fn simulate_benchmark_results(
    candidates: &[CandidateProfile],
    profile: &crate::core::profiles::types::Profile,
) -> Vec<CalibratedResult> {
    let base_cost = profile.budget.max_cost_usd_effective();
    let base_budget = profile.budget.max_context_tokens_effective() as f64;

    candidates
        .iter()
        .map(|c| {
            let budget_ratio = c.budget_tokens as f64 / base_budget;
            let compression_factor = match c.compression.as_str() {
                "lossless" => 1.0,
                "balanced" => 0.65,
                "aggressive" => 0.40,
                _ => 0.75,
            };
            let cost = base_cost * budget_ratio * compression_factor;
            let quality = match c.compression.as_str() {
                "lossless" => 0.98 - (1.0 - c.reuse_threshold) * 0.05,
                "balanced" => 0.96 - (1.0 - c.reuse_threshold) * 0.08,
                "aggressive" => 0.91 - (1.0 - c.reuse_threshold) * 0.15,
                _ => 0.94,
            };
            let latency = 100.0 * budget_ratio;

            CalibratedResult {
                candidate: c.clone(),
                cost_per_task: cost,
                mean_quality: quality.clamp(0.0, 1.0),
                mean_latency_ms: latency,
                pass_rate: if quality >= 0.95 { 1.0 } else { 0.8 },
                quality_floor_met: quality >= 0.95,
            }
        })
        .collect()
}

fn parse_flag_f64(args: &[String], flag: &str) -> Option<f64> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_flag_usize(args: &[String], flag: &str) -> Option<usize> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn print_help() {
    eprintln!(
        "\
lean-ctx calibrate — find the optimal Performance Profile

USAGE:
    lean-ctx calibrate [PROFILE] [OPTIONS]

ARGS:
    PROFILE             Profile name (default: current active profile)

OPTIONS:
    --quality-floor N   Minimum quality score 0.0-1.0 (default: 0.95)
    --max-candidates N  Maximum candidate profiles to test (default: 12)
    --export-spec       Print BenchmarkSpec JSON to stdout
    -h, --help          Show this help

EXAMPLES:
    lean-ctx calibrate
    lean-ctx calibrate coder --quality-floor 0.90
    lean-ctx calibrate monorepo --max-candidates 20 --export-spec > spec.json

NOTE:
    This is calibrator v0 — uses simulated benchmarks. Real agent
    invocation will be available with Agent Connector integration."
    );
}
