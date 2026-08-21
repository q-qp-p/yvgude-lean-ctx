use super::traits::{AgentConnector, AgentInfo, TaskRequest, TaskResult, TokenUsage};
use std::process::Command;
use std::time::Instant;

pub(crate) struct CodexConnector {
    info: AgentInfo,
}
impl CodexConnector {
    pub(crate) fn new(info: AgentInfo) -> Self {
        Self { info }
    }
}

impl AgentConnector for CodexConnector {
    fn info(&self) -> AgentInfo {
        self.info.clone()
    }
    fn health_check(&self) -> anyhow::Result<bool> {
        if !self.info.available {
            return Ok(false);
        }
        Ok(Command::new(&self.info.path)
            .arg("--version")
            .output()?
            .status
            .success())
    }
    fn execute(&self, request: &TaskRequest) -> anyhow::Result<TaskResult> {
        let start = Instant::now();
        let mut cmd = Command::new(&self.info.path);
        cmd.arg("exec")
            .arg("--approve-for-me")
            .arg(&request.prompt)
            .current_dir(&request.working_dir);
        if let Some(model) = &request.model {
            cmd.arg("-m").arg(model);
        }
        let output = cmd.output()?;
        Ok(TaskResult {
            task_id: request.id.clone(),
            agent: "codex".into(),
            model: request.model.clone().unwrap_or_default(),
            success: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
            tokens_used: parse_codex_usage(&output.stdout),
        })
    }
    fn name(&self) -> &'static str {
        "codex"
    }
}

fn parse_codex_usage(stdout: &[u8]) -> Option<TokenUsage> {
    let text = String::from_utf8_lossy(stdout);
    let start = text.find("\"usage\"")?;
    let block_start = text[start..].find('{')?;
    let rest = &text[start + block_start..];
    let block_end = rest.find('}')?;
    let val: serde_json::Value = serde_json::from_str(&rest[..=block_end]).ok()?;
    Some(TokenUsage {
        input_tokens: val["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: val["output_tokens"].as_u64().unwrap_or(0),
        cache_read_tokens: val["cache_read_tokens"].as_u64().unwrap_or(0),
        cache_write_tokens: val["cache_write_tokens"].as_u64().unwrap_or(0),
    })
}
