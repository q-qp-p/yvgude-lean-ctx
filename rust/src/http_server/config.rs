use std::path::PathBuf;

use anyhow::{Result, anyhow};
use rmcp::transport::StreamableHttpServerConfig;

const MAX_ID_LEN: usize = 64;

pub(super) fn sanitize_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "default".to_string();
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(MAX_ID_LEN)
        .collect();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

#[derive(Clone, Debug)]
pub struct HttpServerConfig {
    pub host: String,
    pub port: u16,
    pub project_root: PathBuf,
    pub auth_token: Option<String>,
    pub stateful_mode: bool,
    pub json_response: bool,
    pub disable_host_check: bool,
    pub allowed_hosts: Vec<String>,
    pub max_body_bytes: usize,
    pub max_concurrency: usize,
    pub max_rps: u32,
    pub rate_burst: u32,
    pub request_timeout_ms: u64,
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            project_root,
            auth_token: None,
            stateful_mode: false,
            json_response: true,
            disable_host_check: false,
            allowed_hosts: Vec::new(),
            max_body_bytes: 2 * 1024 * 1024,
            max_concurrency: 32,
            max_rps: 50,
            rate_burst: 100,
            request_timeout_ms: 30_000,
        }
    }
}

impl HttpServerConfig {
    pub fn validate(&self) -> Result<()> {
        let host = self.host.trim().to_lowercase();
        let is_loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
        if !is_loopback && self.auth_token.as_deref().unwrap_or("").is_empty() {
            return Err(anyhow!(
                "Refusing to bind to host='{host}' without auth. Provide --auth-token (or bind to 127.0.0.1)."
            ));
        }
        Ok(())
    }

    pub fn effective_auth_token(&self) -> Option<String> {
        if let Some(ref token) = self.auth_token
            && !token.is_empty()
        {
            return Some(token.clone());
        }
        let host = self.host.trim().to_lowercase();
        let is_loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
        if is_loopback {
            let auto_token = crate::core::session_token::generate_token();
            eprintln!(
                "[lean-ctx] Auto-generated auth token for loopback: {auto_token}\n\
                 Pass as Bearer token or set --auth-token explicitly."
            );
            Some(auto_token)
        } else {
            None
        }
    }

    pub(super) fn mcp_http_config(&self) -> StreamableHttpServerConfig {
        let mut cfg = StreamableHttpServerConfig::default()
            .with_stateful_mode(self.stateful_mode)
            .with_json_response(self.json_response);

        if self.disable_host_check {
            tracing::warn!(
                "⚠ --disable-host-check is active: DNS rebinding protection is OFF. \
                 Do NOT use this in production or on non-loopback interfaces."
            );
            cfg = cfg.disable_allowed_hosts();
            return cfg;
        }

        if !self.allowed_hosts.is_empty() {
            cfg = cfg.with_allowed_hosts(self.allowed_hosts.clone());
            return cfg;
        }

        // Keep rmcp's secure loopback defaults; also allow the configured host (if it's loopback).
        let host = self.host.trim();
        if host == "127.0.0.1" || host == "localhost" || host == "::1" {
            cfg.allowed_hosts.push(host.to_string());
        }

        cfg
    }
}
