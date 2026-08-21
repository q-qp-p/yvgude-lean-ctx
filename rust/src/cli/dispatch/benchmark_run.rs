use crate::core::agent_connector;
use crate::core::benchmark_spec::profile_bridge;
use crate::core::profiles;

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
        return 1;
    };

    let suite = if suite_name == "coding-v1" {
        profile_bridge::default_coding_suite()
    } else {
        eprintln!("error: benchmark suite '{suite_name}' not found");
        return 1;
    };

    let mut spec = match profile_bridge::create_spec(&profile_name, suite) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    spec.configuration.repeats = repeats;

    let connectors = agent_connector::detect_and_create_connectors();
    if let Some(agent) = requested_agent {
        if !connectors
            .iter()
            .any(|c| connector_matches(agent, c.name()))
        {
            eprintln!("error: agent '{agent}' was not detected");
            return 1;
        }
    }

    if args.iter().any(|a| a == "--dry-run") {
        if let Ok(json) = serde_json::to_string_pretty(&spec) {
            println!("{json}");
        }
        return 0;
    }

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
    eprintln!(
        "  Local Runner not yet wired \u{2014} use `lean-ctx calibrate` for simulated benchmarks"
    );
    0
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
    --dry-run           Print BenchmarkSpec JSON and exit
    -h, --help          Show this help

EXAMPLES:
    lean-ctx benchmark-run
    lean-ctx benchmark-run codex --profile monorepo --repeats 3"
    );
}
