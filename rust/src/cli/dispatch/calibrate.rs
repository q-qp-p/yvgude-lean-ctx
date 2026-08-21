use crate::core::agent_connector;
use crate::core::agent_connector::traits::AgentConnector;
use crate::core::benchmark_spec::profile_bridge;
use crate::core::calibrator::candidate::CandidateProfile;
use crate::core::calibrator::pareto::CalibratedResult;
use crate::core::calibrator::{self, CalibrationConfig};
use crate::core::local_runner::runner::{LocalRunner, RunConfig};
use crate::core::profiles;
use std::path::PathBuf;

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
    let live = args.iter().any(|arg| arg == "--live");
    let requested_agent = parse_flag_str(args, "--agent");

    eprintln!("lean-ctx calibrate");
    eprintln!("  Profile:        {profile_name}");
    eprintln!("  Quality floor:  {:.0}%", quality_floor * 100.0);
    eprintln!("  Max candidates: {max_candidates}");
    eprintln!(
        "  Mode:           {}",
        if live { "live" } else { "simulated" }
    );
    if let Some(agent) = &requested_agent {
        eprintln!("  Agent:          {agent}");
    }
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

    let results = if live {
        let working_dir = match parse_working_dir(args) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("error: {error}");
                return 2;
            }
        };
        let connectors = agent_connector::detect_and_create_connectors();
        let Some(connector) = connectors.into_iter().find(|connector| {
            connector.info().available
                && requested_agent
                    .as_deref()
                    .is_none_or(|agent| connector_matches(agent, connector.name()))
        }) else {
            if let Some(agent) = requested_agent {
                eprintln!("error: agent '{agent}' was not detected");
            } else {
                eprintln!("error: no supported agent detected for live calibration");
            }
            return 2;
        };

        eprintln!("  Using agent:    {}", connector.name());
        eprintln!();
        match run_live_benchmark_results(&candidates, profile_name, &config, working_dir, connector)
        {
            Ok(results) => results,
            Err(error) => {
                eprintln!("error: live calibration failed: {error}");
                return 2;
            }
        }
    } else {
        simulate_benchmark_results(&candidates, &profile)
    };

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

/// Offline simulation — used when --live is not passed.
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

fn run_live_benchmark_results(
    candidates: &[CandidateProfile],
    profile_name: &str,
    calibration_config: &CalibrationConfig,
    working_dir: PathBuf,
    connector: Box<dyn AgentConnector>,
) -> anyhow::Result<Vec<CalibratedResult>> {
    let agent_name = connector.name().to_owned();
    let runner = LocalRunner::new(
        RunConfig {
            agent_name,
            profile_name: profile_name.to_owned(),
            suite_name: Some("coding-v1".into()),
            timeout_override_ms: None,
            working_dir,
            repeats: 1,
        },
        connector,
    );

    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            eprintln!(
                "Running candidate {}/{}: {}...",
                index + 1,
                candidates.len(),
                candidate.label
            );
            let mut spec =
                profile_bridge::create_spec(profile_name, profile_bridge::default_coding_suite())?;
            spec.id = format!("{}-{}", spec.id, candidate.id);
            spec.name = format!("{} ({})", spec.name, candidate.label);
            spec.configuration.quality_floor = calibration_config.quality_floor;

            let benchmark = runner.run(&spec)?;
            Ok(CalibratedResult::from_benchmark_result(
                candidate.clone(),
                &benchmark,
            ))
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

fn parse_flag_str(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_working_dir(args: &[String]) -> Result<PathBuf, String> {
    let path = match parse_flag_str(args, "--working-dir") {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir()
            .map_err(|error| format!("cannot determine working directory: {error}"))?,
    };
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!(
            "working directory '{}' does not exist",
            path.display()
        ))
    }
}

fn connector_matches(requested: &str, detected: &str) -> bool {
    let requested = requested.to_lowercase();
    let detected = detected.to_lowercase();
    detected.contains(&requested) || requested.contains(&detected)
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
    --live              Run each candidate against a detected local agent
    --agent NAME        Agent to use with --live (default: first detected)
    --working-dir PATH  Agent working directory with --live (default: current directory)
    --export-spec       Print BenchmarkSpec JSON to stdout
    -h, --help          Show this help

EXAMPLES:
    lean-ctx calibrate
    lean-ctx calibrate coder --quality-floor 0.90
    lean-ctx calibrate coder --live --agent codex --working-dir .
    lean-ctx calibrate monorepo --max-candidates 20 --export-spec > spec.json

NOTE:
    Without --live, calibration uses simulated benchmark results."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::local_runner::runner::MockConnector;

    fn candidate() -> CandidateProfile {
        CandidateProfile {
            id: "candidate-001".into(),
            label: "leanctx/balanced/32k/r85".into(),
            budget_tokens: 32_000,
            compression: "balanced".into(),
            reuse_threshold: 0.85,
            capability_variant: "leanctx".into(),
        }
    }

    #[test]
    fn live_calibration_runs_mock_connector_for_each_candidate() {
        let config = CalibrationConfig::default();
        let results = run_live_benchmark_results(
            &[candidate()],
            "coder",
            &config,
            std::env::current_dir().expect("test working directory"),
            Box::new(MockConnector::new(true)),
        )
        .expect("mock connector must complete live calibration");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].candidate.id, "candidate-001");
        assert_eq!(results[0].pass_rate, 1.0);
        assert!(results[0].quality_floor_met);
    }
}
