//! `lean-ctx evidence` — evidence-bundle commands.

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use zip::ZipArchive;

use super::evidence_realworld;
use super::evidence_report;
use super::evidence_workflow::{self, ScenarioType, WorkflowArgs};

/// Arguments for evidence-bundle operations.
#[derive(Debug, Parser)]
#[command(
    name = "lean-ctx evidence",
    about = "Generate or inspect evidence bundles"
)]
struct EvidenceCommand {
    #[command(subcommand)]
    command: EvidenceSubcommand,
}

#[derive(Debug, Subcommand)]
enum EvidenceSubcommand {
    /// Compare providers and generate an evidence bundle.
    Run(EvidenceRunCommand),
    /// Run a strict multi-step workflow evidence scenario with USD costs.
    Workflow(EvidenceWorkflowCommand),
    /// List an evidence bundle's files and display its manifest.
    /// Real-world multi-turn evidence: 5 sequential API calls per arm with provider cache tracking.
    Realworld(EvidenceRealworldCommand),
    /// Render a customer-readable HTML report from real-world evidence JSON.
    Report(EvidenceReportCommand),
    Inspect(EvidenceInspectCommand),
}

#[derive(Debug, Args)]
struct EvidenceRunCommand {}

#[derive(Debug, Args)]
struct EvidenceWorkflowCommand {
    /// Target directory to analyze (defaults to current directory).
    #[arg(long, default_value = ".")]
    target: PathBuf,

    /// Output directory for evidence artifacts.
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Scenario: bugfix (small), review (medium), refactor (large), all.
    #[arg(long, default_value = "all")]
    scenario: String,

    /// LLM API endpoint (OpenAI-compatible).
    #[arg(long, default_value = "https://api.openai.com/v1/chat/completions")]
    endpoint: String,

    /// Model name for LLM comparison.
    #[arg(long, default_value = "gpt-4o-mini")]
    model: String,

    /// API key (or set OPENAI_API_KEY / ANTHROPIC_API_KEY env var).
    #[arg(long)]
    api_key: Option<String>,

    /// Custom system prompt for code review.
    #[arg(long)]
    system_prompt: Option<String>,
}

#[derive(Debug, Args)]
struct EvidenceRealworldCommand {
    /// Target directory to analyze.
    #[arg(long, default_value = ".")]
    target: PathBuf,

    /// Output directory for evidence artifacts.
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// LLM API endpoint (OpenAI-compatible).
    #[arg(long, default_value = "https://api.openai.com/v1/chat/completions")]
    endpoint: String,

    /// Model name.
    #[arg(long, default_value = "gpt-4o-mini")]
    model: String,

    /// API key (or set OPENAI_API_KEY env var).
    #[arg(long)]
    api_key: Option<String>,

    /// Assert PROOF-DOCTRINE VERIFY gates after the run (exit 1 on FAIL).
    #[arg(long)]
    quality_gate: bool,
}

#[derive(Debug, Args)]
struct EvidenceReportCommand {
    /// Path to a realworld-result.json file produced by `evidence realworld`.
    #[arg(long, value_name = "JSON", value_hint = clap::ValueHint::FilePath)]
    input: PathBuf,

    /// Destination for the self-contained HTML report.
    #[arg(long, value_name = "HTML", value_hint = clap::ValueHint::FilePath)]
    output: PathBuf,
}
#[derive(Debug, Args)]
struct EvidenceInspectCommand {
    /// Path to the evidence-bundle ZIP archive.
    #[arg(value_name = "BUNDLE_PATH", value_hint = clap::ValueHint::FilePath)]
    bundle_path: PathBuf,
}

/// Parse, execute, print, and return the process status for evidence commands.
pub(crate) fn run(args: &[String]) -> i32 {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("lean-ctx evidence".to_string());
    argv.extend(args.iter().cloned());

    let command = match EvidenceCommand::try_parse_from(argv) {
        Ok(command) => command,
        Err(error) => {
            let code = error.exit_code();
            eprint!("{error}");
            return code;
        }
    };

    match command.command {
        EvidenceSubcommand::Run(_) => {
            println!(
                "Provider comparison evidence generation is not available yet.\n\nUsage: lean-ctx evidence run"
            );
            0
        }
        EvidenceSubcommand::Workflow(cmd) => match execute_workflow(&cmd) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("evidence workflow: {error:#}");
                1
            }
        },
        EvidenceSubcommand::Realworld(cmd) => match execute_realworld(&cmd) {
            Ok(exit_code) => exit_code,
            Err(error) => {
                eprintln!("evidence realworld: {error:#}");
                1
            }
        },
        EvidenceSubcommand::Report(cmd) => {
            match evidence_report::render_file(&cmd.input, &cmd.output) {
                Ok(()) => {
                    println!("Evidence report written to {}", cmd.output.display());
                    0
                }
                Err(error) => {
                    eprintln!("evidence report: {error:#}");
                    1
                }
            }
        }
        EvidenceSubcommand::Inspect(command) => match inspect(&command.bundle_path) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("evidence inspect: {error:#}");
                1
            }
        },
    }
}

fn execute_realworld(cmd: &EvidenceRealworldCommand) -> Result<i32> {
    let api_key = cmd
        .api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .context("API key required: --api-key or OPENAI_API_KEY")?;

    let output_dir = cmd.output.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "evidence-realworld-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ))
    });

    let target = if cmd.target == Path::new(".") {
        std::env::current_dir()?
    } else {
        fs::canonicalize(&cmd.target)?
    };

    fs::create_dir_all(&output_dir)?;

    let args = evidence_realworld::RealWorldArgs {
        target_dir: target,
        output_dir: output_dir.clone(),
        endpoint: cmd.endpoint.clone(),
        model: cmd.model.clone(),
        api_key,
        quality_gate: cmd.quality_gate,
    };

    let result = evidence_realworld::execute_realworld_evidence(&args)?;

    // Write full result JSON
    let json = serde_json::to_string_pretty(&result)?;
    fs::write(output_dir.join("realworld-result.json"), &json)?;

    // Write per-turn details
    for arm in [&result.baseline, &result.treatment] {
        let arm_dir = output_dir.join(&arm.arm);
        fs::create_dir_all(&arm_dir)?;
        for turn in &arm.turns {
            let fname = format!("turn-{}-{}.json", turn.turn, turn.name);
            let turn_json = serde_json::to_string_pretty(turn)?;
            fs::write(arm_dir.join(fname), turn_json)?;
        }
    }

    // Print summary
    println!();
    println!("═══ RESULT ═══");
    println!("Token Savings:  {:.1}%", result.savings_tokens_pct);
    println!(
        "Cost Savings:   {:.1}% (${:.5})",
        result.savings_cost_pct, result.savings_usd
    );
    println!(
        "Baseline Total: ${:.5} ({} prompt tok)",
        result.baseline.total_real_cost_usd, result.baseline.total_prompt_tokens
    );
    println!(
        "Treatment Total:${:.5} ({} prompt tok)",
        result.treatment.total_real_cost_usd, result.treatment.total_prompt_tokens
    );
    println!();
    println!("Provider Cache Effects:");
    println!(
        "  Baseline cached:  {} tokens across 5 turns",
        result.baseline.total_cached_tokens
    );
    println!(
        "  Treatment cached: {} tokens across 5 turns",
        result.treatment.total_cached_tokens
    );
    println!();
    println!("Artifacts: {}", output_dir.display());
    println!("Methodology: {}", result.methodology.description);

    if let Some(ref gate) = result.quality_gate {
        println!();
        println!("Quality Gate: {}", gate.verdict);
        for reason in &gate.reasons {
            println!("  - {reason}");
        }
        let gate_json = serde_json::to_string_pretty(gate)?;
        fs::write(output_dir.join("quality-gate.json"), gate_json)?;
        if gate.verdict == "FAIL" {
            return Ok(1);
        }
    }

    Ok(0)
}

fn execute_workflow(cmd: &EvidenceWorkflowCommand) -> Result<()> {
    let api_key = cmd
        .api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .unwrap_or_else(|| "skip".to_string());

    let output_dir = cmd.output.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "evidence-workflow-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ))
    });

    let target = if cmd.target == Path::new(".") {
        std::env::current_dir().context("get cwd")?
    } else {
        fs::canonicalize(&cmd.target)
            .with_context(|| format!("resolve target: {}", cmd.target.display()))?
    };

    let args = WorkflowArgs {
        target_dir: target,
        output_dir: output_dir.clone(),
        endpoint: cmd.endpoint.clone(),
        model: cmd.model.clone(),
        api_key,
        system_prompt: cmd.system_prompt.clone().unwrap_or_default(),
        scenario: ScenarioType::from_str(&cmd.scenario),
    };

    let result = evidence_workflow::execute_workflow_evidence(&args)?;

    // Write evidence artifacts
    fs::create_dir_all(&output_dir)?;

    for scenario in &result.scenarios {
        let scenario_dir = output_dir.join("scenarios").join(&scenario.scenario);
        let turns_dir = scenario_dir.join("turns");
        fs::create_dir_all(&turns_dir)?;

        for turn in &scenario.turns {
            let turn_path = turns_dir.join(format!("turn-{}.json", turn.turn));
            fs::write(&turn_path, serde_json::to_string_pretty(turn)?)?;
        }

        fs::write(
            scenario_dir.join("summary.json"),
            serde_json::to_string_pretty(scenario)?,
        )?;

        if let Some(ref q) = scenario.quality {
            fs::write(
                scenario_dir.join("quality-score.json"),
                serde_json::to_string_pretty(q)?,
            )?;
        }
    }

    fs::write(
        output_dir.join("cost-comparison.json"),
        serde_json::to_string_pretty(&result.cost_report)?,
    )?;
    fs::write(
        output_dir.join("workflow-result.json"),
        serde_json::to_string_pretty(&result)?,
    )?;

    // Generate evidence bundle ZIP
    let bundle_path = output_dir.join("evidence-bundle.zip");
    create_workflow_bundle(&output_dir, &bundle_path)?;

    // Print summary
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  Strict Workflow Evidence Complete                           ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                                                              ║");
    println!(
        "║  Scenarios:          {:>3}                                      ║",
        result.scenarios.len()
    );
    println!(
        "║  Total baseline:     {:>8} tokens                         ║",
        result.aggregate_baseline_tokens
    );
    println!(
        "║  Total treatment:    {:>8} tokens                         ║",
        result.aggregate_treatment_tokens
    );
    println!(
        "║  Total savings:      {:>6.1}%                                ║",
        result.aggregate_savings_pct
    );
    println!("║                                                              ║");
    println!("║  Output: {:<50}║", output_dir.display());
    println!("║                                                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Per-scenario breakdown
    println!("Per-scenario breakdown:");
    for s in &result.scenarios {
        println!(
            "  {:<10} {:>7} -> {:>7} tokens ({:.1}% saved, {:.1}x)",
            s.scenario,
            s.total_baseline_tokens,
            s.total_treatment_tokens,
            s.savings_pct,
            s.reduction_factor
        );
        for t in &s.turns {
            let cap = if t.shell_was_capped { " [capped]" } else { "" };
            println!(
                "    Turn {}: {:<18} +{:>6} -> +{:>6} (acc: {:>7} -> {:>6}){}",
                t.turn,
                t.name,
                t.new_content_baseline_tokens,
                t.new_content_treatment_tokens,
                t.accumulated_baseline_tokens,
                t.accumulated_treatment_tokens,
                cap
            );
        }
        if let Some(ref q) = s.quality {
            println!(
                "    Quality: {} (overlap {:.0}%, issues {}/{})",
                q.verdict, q.overlap_pct, q.treatment_issues_found, q.baseline_issues_found
            );
        }
    }

    Ok(())
}

fn create_workflow_bundle(dir: &Path, bundle_path: &Path) -> Result<()> {
    let file = File::create(bundle_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    let walker = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.file_name() != "evidence-bundle.zip");

    for entry in walker {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(dir)?;
            zip.start_file(rel.to_string_lossy(), options)?;
            let content = fs::read(entry.path())?;
            std::io::Write::write_all(&mut zip, &content)?;
        }
    }

    let manifest = serde_json::json!({
        "bundle": "evidence-workflow-strict",
        "version": "2.0.0",
        "lean_ctx_version": env!("CARGO_PKG_VERSION"),
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "methodology": {
            "multi_turn": true,
            "shell_cap_lines": 500,
            "fair_baseline": "Shell output capped to last 500 lines (realistic agent view)",
            "quality_scored": true,
            "cost_projected": true
        }
    });
    zip.start_file("manifest.json", options)?;
    let manifest_bytes = serde_json::to_string_pretty(&manifest)?;
    std::io::Write::write_all(&mut zip, manifest_bytes.as_bytes())?;

    zip.finish()?;
    Ok(())
}

fn inspect(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("read ZIP archive")?;
    let mut names = Vec::with_capacity(archive.len());
    let mut manifest = None;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("read ZIP entry {index}"))?;
        let name = entry.name().to_string();
        if name == "manifest.json" {
            let mut contents = String::new();
            entry
                .read_to_string(&mut contents)
                .context("read manifest.json")?;
            manifest = Some(contents);
        }
        names.push(name);
    }

    let manifest = manifest.context("missing manifest.json")?;
    println!("Evidence bundle: {}", path.display());
    println!("Files:");
    for name in names {
        println!("  {name}");
    }
    println!("Manifest:");
    println!("{manifest}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn inspect_reads_manifest_and_lists_entries() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(b"{\"bundle\":\"evidence-bundle\"}").unwrap();
        zip.start_file("audit/trail.jsonl", options).unwrap();
        zip.write_all(b"entry\n").unwrap();
        let path = std::env::temp_dir().join(format!(
            "lean-ctx-evidence-inspect-{}.zip",
            std::process::id()
        ));
        std::fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();

        assert!(inspect(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn inspect_requires_manifest() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("audit/trail.jsonl", options).unwrap();
        zip.write_all(b"entry\n").unwrap();
        let path = std::env::temp_dir().join(format!(
            "lean-ctx-evidence-no-manifest-{}.zip",
            std::process::id()
        ));
        std::fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();

        assert!(inspect(&path).is_err());
        let _ = std::fs::remove_file(path);
    }
}
