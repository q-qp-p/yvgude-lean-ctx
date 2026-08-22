use crate::core::agent_connector;
use crate::core::agent_connector::traits::AgentConnector;
use crate::core::benchmark_spec::types::BenchmarkSpecV1;
use crate::core::benchmark_spec::{profile_bridge, report};
use crate::core::local_runner::runner::{LocalRunner, RunConfig};
use crate::core::profiles;
use std::path::PathBuf;

pub(crate) fn cmd_benchmark_run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return 0;
    }

    let profile_name = parse_flag_str(args, "--profile").unwrap_or_else(|| "coder".into());
    let suite_name = parse_flag_str(args, "--suite").unwrap_or_else(|| "coding-v1".into());
    let repeats = parse_flag_u32(args, "--repeats").unwrap_or(1);
    let requested_agent = first_positional(args);

    eprintln!("lean-ctx benchmark-run");
    eprintln!("  Profile: {profile_name}");
    eprintln!("  Suite:   {suite_name}");
    eprintln!("  Repeats: {repeats}");
    if let Some(agent) = requested_agent {
        eprintln!("  Agent:   {agent}");
    }
    eprintln!();

    let Some(_profile) = profiles::load_profile(&profile_name) else {
        eprintln!("error: profile '{profile_name}' not found");
        return 2;
    };

    let suite = if suite_name == "coding-v1" {
        profile_bridge::default_coding_suite()
    } else {
        eprintln!("error: benchmark suite '{suite_name}' not found");
        return 2;
    };

    let mut spec = match profile_bridge::create_spec(&profile_name, suite) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    spec.configuration.repeats = repeats;

    if args.iter().any(|a| a == "--dry-run") {
        if let Ok(json) = dry_run_spec_json(&spec) {
            println!("{json}");
        }
        return 0;
    }

    let format = match parse_flag_str(args, "--format").as_deref() {
        None | Some("terminal") => "terminal",
        Some("json") => "json",
        Some(value) => {
            eprintln!("error: unsupported report format '{value}' (use terminal or json)");
            return 2;
        }
    };
    let working_dir = match parse_working_dir(args) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    let connectors = agent_connector::detect_and_create_connectors();

    eprintln!("  Detected agents:");
    for c in &connectors {
        let info = c.info();
        let st = if info.available {
            "ready"
        } else {
            "unavailable"
        };
        eprintln!(
            "    {} {} [{st}]",
            display_agent_name(c.name()),
            info.version.as_deref().unwrap_or("?")
        );
    }
    eprintln!();
    let Some(connector) = connectors.into_iter().find(|connector| {
        requested_agent.is_none_or(|agent| connector_matches(agent, connector.name()))
    }) else {
        let agent = requested_agent.unwrap_or("supported agent");
        eprintln!("error: agent '{agent}' was not detected");
        return 2;
    };
    let config = RunConfig {
        agent_name: connector.name().to_owned(),
        profile_name,
        suite_name: Some(suite_name),
        timeout_override_ms: None,
        working_dir,
        repeats,
    };
    match run_with_connector(&spec, config, connector, format) {
        Ok((exit_code, output)) => {
            println!("{output}");
            exit_code
        }
        Err(error) => {
            eprintln!("error: benchmark setup failed: {error}");
            2
        }
    }
}

fn dry_run_spec_json(spec: &BenchmarkSpecV1) -> serde_json::Result<String> {
    serde_json::to_string_pretty(spec)
}

fn run_with_connector(
    spec: &BenchmarkSpecV1,
    config: RunConfig,
    connector: Box<dyn AgentConnector>,
    format: &str,
) -> anyhow::Result<(i32, String)> {
    let result = LocalRunner::new(config, connector).run(spec)?;
    let output = if format == "json" {
        report::format_json(&result)
    } else {
        report::format_terminal(&result)
    };
    let exit_code = i32::from(!result.outcomes.iter().all(|outcome| outcome.passed));
    Ok((exit_code, output))
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

fn connector_matches(r: &str, c: &str) -> bool {
    let r = r.to_lowercase();
    let c = c.to_lowercase();
    c.contains(&r) || r.contains(&c)
}
fn display_agent_name(n: &str) -> &str {
    match n {
        "codex" => "Codex",
        "claude" | "claude-code" => "Claude Code",
        "cursor" => "Cursor",
        _ => n,
    }
}

fn connector_names(connectors: &[Box<dyn agent_connector::traits::AgentConnector>]) -> String {
    connectors
        .iter()
        .filter(|c| c.info().available)
        .map(|c| display_agent_name(c.name()).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn first_positional(args: &[String]) -> Option<&str> {
    let flags = [
        "--profile",
        "--suite",
        "--working-dir",
        "--repeats",
        "--output",
        "--format",
    ];
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if flags.contains(&arg.as_str()) {
            skip = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return Some(arg.as_str());
    }
    None
}

fn parse_flag_str(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
fn parse_flag_u32(args: &[String], flag: &str) -> Option<u32> {
    parse_flag_str(args, flag).and_then(|v| v.parse().ok())
}

#[allow(dead_code)]
fn _connector_names_used(c: &[Box<dyn agent_connector::traits::AgentConnector>]) -> String {
    connector_names(c)
}

fn print_help() {
    eprintln!(
        "\
lean-ctx benchmark-run \u{2014} run a benchmark against a coding agent

USAGE:
    lean-ctx benchmark-run [AGENT] [OPTIONS]

OPTIONS:
    --profile NAME      Performance Profile (default: active)
    --suite NAME        Benchmark suite (default: coding-v1)
    --repeats N         Repetitions (default: 1)
    --working-dir PATH  Agent working directory (default: current directory)
    --format FORMAT     Report format: terminal or json (default: terminal)
    --dry-run           Print BenchmarkSpec JSON and exit
    -h, --help          Show this help

EXAMPLES:
    lean-ctx benchmark-run
    lean-ctx benchmark-run codex --profile monorepo --repeats 3"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::local_runner::runner::MockConnector;

    fn test_spec() -> BenchmarkSpecV1 {
        profile_bridge::create_spec("coder", profile_bridge::default_coding_suite())
            .expect("builtin coder profile must produce a benchmark spec")
    }

    fn runner_config() -> RunConfig {
        RunConfig {
            agent_name: "mock".into(),
            profile_name: "coder".into(),
            suite_name: Some("coding-v1".into()),
            timeout_override_ms: None,
            working_dir: std::env::current_dir().expect("test working directory"),
            repeats: 1,
        }
    }

    #[test]
    fn dry_run_prints_json_spec() {
        let spec = test_spec();
        let output = dry_run_spec_json(&spec).expect("spec must serialize for CLI output");
        let parsed: BenchmarkSpecV1 =
            serde_json::from_str(&output).expect("dry-run output must be JSON");

        assert_eq!(parsed.id, spec.id);
        assert_eq!(parsed.suite.tasks.len(), spec.suite.tasks.len());
    }

    #[test]
    fn unevaluated_suite_uses_local_runner_without_claiming_success() {
        let spec = test_spec();

        let (exit_code, output) = run_with_connector(
            &spec,
            runner_config(),
            Box::new(MockConnector::new(true)),
            "terminal",
        )
        .expect("mock connector must run through LocalRunner");

        let legacy_message = ["Local Runner not yet", "wired"].join(" ");
        assert_eq!(exit_code, 1);
        assert!(output.contains("Benchmark Result"));
        assert!(!output.contains(&legacy_message));
    }

    #[test]
    fn failed_tasks_return_exit_code_one() {
        let spec = test_spec();

        let (exit_code, _) = run_with_connector(
            &spec,
            runner_config(),
            Box::new(MockConnector::new(false)),
            "terminal",
        )
        .expect("mock connector must run through LocalRunner");

        assert_eq!(exit_code, 1);
    }
}
