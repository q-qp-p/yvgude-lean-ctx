//! Development-only organization integration commands.

use std::io::{self, Write};
use std::time::Duration;

use crate::core::{config::EnterpriseConfig, theme};

const DEFAULT_GATEWAY_URL: &str = "https://api.leanctx.com/v1";

#[derive(Default)]
struct InitArgs {
    url: Option<String>,
    token: Option<String>,
    force: bool,
}

pub(crate) fn cmd_enterprise(args: &[String]) {
    if std::env::var("LEAN_CTX_EXPERIMENTAL_ENTERPRISE").as_deref() != Ok("1") {
        eprintln!(
            "Organization operations are Research and unavailable in the public LeanCTX Runtime. \\
             Set LEAN_CTX_EXPERIMENTAL_ENTERPRISE=1 only for a local development evaluation."
        );
        return;
    }

    let result = match args.first().map(String::as_str) {
        Some("init") => run_init(&args[1..]),
        Some("status") => run_status(),
        Some("--help" | "-h") | None => {
            print_help();
            return;
        }
        _ => Err("unknown command; use `lean-ctx enterprise --help`".into()),
    };

    if let Err(error) = result {
        print_error(&error);
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "Usage: LEAN_CTX_EXPERIMENTAL_ENTERPRISE=1 lean-ctx enterprise <COMMAND>\n\nDevelopment-only commands:\n  init    Configure a local evaluation connection\n  status  Show the local evaluation connection status\n\nThis is Research, not a public LeanCTX Enterprise service.\n\nOptions:\n  -h, --help  Print this help"
    );
}

fn run_status() -> Result<(), String> {
    let config = crate::core::config::Config::load_global();
    let enterprise = &config.enterprise;
    if enterprise.disabled {
        println!("Organization integration (development-only): disabled");
        return Ok(());
    }

    let Some(gateway_url) = enterprise.effective_gateway_url_owned() else {
        println!("Organization integration (development-only): not configured");
        return Ok(());
    };
    let token_status = if enterprise.effective_token().is_some() {
        "configured"
    } else {
        "missing"
    };
    println!("Organization integration (development-only): configured");
    println!("  Gateway URL: {gateway_url}");
    println!("  Instance ID: {}", enterprise.effective_instance_id());
    println!("  Instance token: {token_status}");
    Ok(())
}

fn run_init(args: &[String]) -> Result<(), String> {
    let parsed = parse_init_args(args)?;

    if enterprise_section_exists()? && !parsed.force && !confirm_overwrite()? {
        println!("Enterprise configuration unchanged.");
        return Ok(());
    }

    let url = match parsed.url {
        Some(url) => url,
        None => prompt("Gateway URL", Some(DEFAULT_GATEWAY_URL))?,
    };
    validate_url(&url)?;

    let token = match parsed.token {
        Some(token) => token,
        None => prompt("Instance token", None)?,
    };
    if token.trim().is_empty() {
        return Err("Instance token cannot be empty.".into());
    }

    test_connection(&url, token.trim())?;

    let gateway_url = url.trim_end_matches('/').to_string();
    let instance_token = token.trim().to_string();
    crate::core::config::Config::update_global(|config| {
        config.enterprise = EnterpriseConfig {
            gateway_url: Some(gateway_url.clone()),
            instance_token: Some(instance_token.clone()),
            disabled: false,
            ..config.enterprise.clone()
        };
    })
    .map_err(|error| format!("Could not write enterprise configuration: {error}"))?;

    print_success(&gateway_url);
    Ok(())
}

fn parse_init_args(args: &[String]) -> Result<InitArgs, String> {
    let mut parsed = InitArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--url" => {
                index += 1;
                parsed.url = Some(flag_value(args, index, "--url")?);
            }
            "--token" => {
                index += 1;
                parsed.token = Some(flag_value(args, index, "--token")?);
            }
            "--force" => parsed.force = true,
            unknown => return Err(format!("Unknown enterprise init option: {unknown}")),
        }
        index += 1;
    }
    Ok(parsed)
}

fn flag_value(args: &[String], index: usize, flag: &str) -> Result<String, String> {
    args.get(index)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn validate_url(value: &str) -> Result<(), String> {
    let uri = value
        .parse::<ureq::http::Uri>()
        .map_err(|_| "Gateway URL must be a valid http:// or https:// URL.".to_string())?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.authority().is_none()
        || uri.query().is_some()
    {
        return Err("Gateway URL must be a valid http:// or https:// URL without a query.".into());
    }
    Ok(())
}

fn test_connection(url: &str, token: &str) -> Result<(), String> {
    let health_url = format!("{}/health", url.trim_end_matches('/'));
    let agent = crate::core::http_client::ureq_agent_with_timeouts(
        Some(Duration::from_secs(5)),
        Some(Duration::from_secs(10)),
        Some(Duration::from_secs(10)),
    );

    agent
        .get(&health_url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map(|_| ())
        .map_err(|error| {
            format!(
                "Connection test failed: {error}\nCheck the Gateway URL and instance token, then ensure {health_url} is reachable."
            )
        })
}

fn enterprise_section_exists() -> Result<bool, String> {
    let Some(path) = crate::core::config::Config::path() else {
        return Err("Cannot determine ~/.config/lean-ctx/config.toml path.".into());
    };
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Could not read config.toml: {error}")),
    };
    let config = raw
        .parse::<toml::Table>()
        .map_err(|error| format!("Could not parse config.toml: {error}"))?;
    Ok(config.contains_key("enterprise"))
}

fn confirm_overwrite() -> Result<bool, String> {
    let answer = prompt(
        "Development-only organization configuration exists. Overwrite? [y/N]",
        None,
    )?;
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}

fn prompt(label: &str, default: Option<&str>) -> Result<String, String> {
    match default {
        Some(value) => print!("{label} [{value}]: "),
        None => print!("{label}: "),
    }
    io::stdout()
        .flush()
        .map_err(|error| format!("Could not write prompt: {error}"))?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| format!("Could not read input: {error}"))?;
    let value = value.trim();
    Ok(if value.is_empty() {
        default.unwrap_or_default().to_string()
    } else {
        value.to_string()
    })
}

fn print_success(url: &str) {
    let active_theme = theme::load_theme(&crate::core::config::Config::load_global().theme);
    let green = active_theme.success.fg();
    let reset = theme::rst();
    println!("{green}✓ Development-only organization integration connected to {url}{reset}");
    println!("Local evaluation capabilities enabled:");
    println!("  • Gateway routing");
    println!("  • Authenticated runtime identity");
    println!("  • Local policy evaluation");
}

fn print_error(error: &str) {
    let active_theme = theme::load_theme(&crate::core::config::Config::load_global().theme);
    eprintln!(
        "{red}Error: {error}{reset}",
        red = active_theme.danger.fg(),
        reset = theme::rst()
    );
}

#[cfg(test)]
mod tests {
    use super::validate_url;

    #[test]
    fn validates_gateway_url_format() {
        assert!(validate_url("https://api.leanctx.com/v1").is_ok());
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("api.leanctx.com/v1").is_err());
        assert!(validate_url("ftp://api.leanctx.com/v1").is_err());
        assert!(validate_url("https://api.leanctx.com/v1?debug=true").is_err());
    }
}
