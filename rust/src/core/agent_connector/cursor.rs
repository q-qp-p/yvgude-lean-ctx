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
        let output = Command::new(&self.info.path)
            .arg("agent")
            .arg(&request.prompt)
            .current_dir(&request.working_dir)
            .output()?;
        Ok(TaskResult {
            task_id: request.id.clone(),
            agent: "cursor".into(),
            model: request.model.clone().unwrap_or_default(),
            success: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
            tokens_used: None,
        })
    }
    fn name(&self) -> &'static str {
        "cursor"
    }
}
