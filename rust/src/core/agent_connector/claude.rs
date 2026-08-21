use super::traits::{AgentConnector, AgentInfo, TaskRequest, TaskResult, TokenUsage};
use std::process::Command;
use std::time::Instant;

pub(crate) struct ClaudeConnector {
    info: AgentInfo,
}
impl ClaudeConnector {
    pub(crate) fn new(info: AgentInfo) -> Self {
        Self { info }
    }
}

impl AgentConnector for ClaudeConnector {
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
        cmd.arg("-p")
            .arg(&request.prompt)
            .arg("--output-format")
            .arg("json")
            .current_dir(&request.working_dir);
        if let Some(turns) = request.max_turns {
            cmd.arg("--max-turns").arg(turns.to_string());
        }
        let output = cmd.output()?;
        Ok(TaskResult {
            task_id: request.id.clone(),
            agent: "claude-code".into(),
            model: request.model.clone().unwrap_or_default(),
            success: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
            tokens_used: parse_claude_usage(&output.stdout),
        })
    }
    fn name(&self) -> &'static str {
        "claude-code"
    }
}

fn parse_claude_usage(stdout: &[u8]) -> Option<TokenUsage> {
    let text = String::from_utf8_lossy(stdout);
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    let usage = val.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
        cache_read_tokens: usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
        cache_write_tokens: usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
    })
}
