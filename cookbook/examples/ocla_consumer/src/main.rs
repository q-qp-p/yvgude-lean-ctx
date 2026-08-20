use std::{collections::BTreeMap, error::Error, fs, path::PathBuf};

use clap::{Parser, Subcommand};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(about = "Consume the public OCLA Wire API")]
struct Cli {
    /// OCLA server base URL.
    #[arg(long, default_value = "http://localhost:3333")]
    url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check server health.
    Health,
    /// List server capabilities.
    Capabilities,
    /// Validate an envelope JSON file through the server.
    Validate { file: PathBuf },
    /// Show ledger summary statistics.
    Summary,
}

#[derive(Debug, Deserialize, Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct CapabilitiesResponse {
    version: String,
    capabilities: Vec<Capability>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Capability {
    kind: CapabilityKind,
    api_version: String,
    status: CapabilityStatus,
    limits: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CapabilityKind {
    ObservationHook,
    UsageSink,
    MetricsExporter,
    SavingsLedger,
    IntentClassifier,
    OutcomeTracker,
    CompressionProvider,
    ResponseOptimizer,
    ModelRouter,
    EfficiencyAnalyzer,
    ConfigTuner,
    ExperimentRunner,
    ConnectorScheduler,
    AgentGateway,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CapabilityStatus {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Deserialize, Serialize)]
struct LedgerSummaryResponse {
    events: usize,
    tokens: u64,
    usd: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct CanonicalTokenEnvelopeV1 {
    schema_version: u16,
    context: RequestContext,
    surface: TokenEnvelopeSurface,
    direction: TokenFlowDirection,
    provider: String,
    model: String,
    token_balance: TokenBalanceV1,
    route_ref: Option<String>,
    policy_ref: Option<String>,
    idempotency_key: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct RequestContext {
    request_id: String,
    session_id: String,
    agent_id: String,
    content_ref: String,
    tenant_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TokenEnvelopeSurface {
    Mcp,
    Proxy,
    Shell,
    Agent,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TokenFlowDirection {
    Input,
    Output,
}

#[derive(Debug, Deserialize, Serialize)]
struct TokenBalanceV1 {
    original_tokens: u64,
    materialized_tokens: u64,
    delivered_tokens: u64,
    provider_billed_tokens: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let client = Client::new();

    match cli.command {
        Command::Health => {
            let response: HealthResponse = get_json(&client, &cli.url, "/ocla/v1/health").await?;
            println!("{} ({})", response.status, response.version);
        }
        Command::Capabilities => {
            let response: CapabilitiesResponse =
                get_json(&client, &cli.url, "/ocla/v1/capabilities").await?;
            println!("API version: {}", response.version);
            for capability in response.capabilities {
                println!(
                    "{}: {:?} ({})",
                    serde_json::to_string(&capability.kind)?.trim_matches('"'),
                    capability.status,
                    capability.api_version
                );
            }
        }
        Command::Validate { file } => {
            let body = fs::read_to_string(&file)?;
            let _: CanonicalTokenEnvelopeV1 = serde_json::from_str(&body)?;
            let response = client
                .post(endpoint(&cli.url, "/ocla/v1/envelope"))
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await?;
            let envelope: CanonicalTokenEnvelopeV1 = parse_json_response(response).await?;
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        }
        Command::Summary => {
            let response: LedgerSummaryResponse =
                get_json(&client, &cli.url, "/ocla/v1/ledger/summary").await?;
            println!("events: {}", response.events);
            println!("tokens: {}", response.tokens);
            println!("usd: {:.6}", response.usd);
        }
    }

    Ok(())
}

async fn get_json<T>(client: &Client, base_url: &str, path: &str) -> Result<T, Box<dyn Error>>
where
    T: for<'de> Deserialize<'de>,
{
    let response = client.get(endpoint(base_url, path)).send().await?;
    Ok(parse_json_response(response).await?)
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), path)
}

async fn parse_json_response<T>(response: Response) -> Result<T, Box<dyn Error>>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    let body = response.text().await?;
    if status != StatusCode::OK {
        return Err(format!("OCLA request failed with {status}: {body}").into());
    }
    Ok(serde_json::from_str(&body)?)
}
