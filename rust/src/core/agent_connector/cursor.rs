use super::timeout::run_with_timeout;
use super::traits::{AgentConnector, AgentInfo, TaskRequest, TaskResult};
use std::process::Command;
use std::time::Instant;

pub(crate) struct CursorConnector {
    info: AgentInfo,
}
impl CursorConnector {
    pub(crate) fn new(info: AgentInfo) -> Self {
        Self { info }
    }
}

impl AgentConnector for CursorConnector {
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
        cmd.arg("agent")
            .arg(&request.prompt)
            .current_dir(&request.working_dir);
        let timed_output = run_with_timeout(&mut cmd, request.timeout_ms)?;
        let output = timed_output.output;
        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if timed_output.timed_out {
            stderr.push_str(&format!("task timed out after {}ms", request.timeout_ms));
        }
        Ok(TaskResult {
            task_id: request.id.clone(),
            agent: "cursor".into(),
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
            tokens_used: None,
        })
    }
    fn name(&self) -> &'static str {
        "cursor"
    }
}
