use super::timeout::run_with_timeout;
use super::traits::{
    AgentConnector, AgentInfo, TaskRequest, TaskResult, TokenUsage, apply_profile_environment,
};
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
        apply_profile_environment(&mut cmd, request);
        let timed_output = run_with_timeout(&mut cmd, request.timeout_ms)?;
        let output = timed_output.output;
        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if timed_output.timed_out {
            stderr.push_str(&format!("task timed out after {}ms", request.timeout_ms));
        }
        Ok(TaskResult {
            task_id: request.id.clone(),
            agent: "claude-code".into(),
            model: request.model.clone().unwrap_or_default(),
            success: !timed_output.timed_out && output.status.success(),
            exit_code: if timed_output.timed_out {
                -1
            } else {
                output.status.code().unwrap_or(-1)
            },
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr,
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
