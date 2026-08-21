use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentInfo {
    pub name: String,
    pub version: Option<String>,
    pub path: PathBuf,
    pub capabilities: Vec<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskRequest {
    pub id: String,
    pub prompt: String,
    pub working_dir: PathBuf,
    pub timeout_ms: u64,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub profile_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskResult {
    pub task_id: String,
    pub agent: String,
    pub model: String,
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub tokens_used: Option<TokenUsage>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(crate) struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

pub(crate) trait AgentConnector: Send + Sync {
    fn info(&self) -> AgentInfo;
    fn health_check(&self) -> anyhow::Result<bool>;
    fn execute(&self, request: &TaskRequest) -> anyhow::Result<TaskResult>;
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_request_roundtrip() {
        let req = TaskRequest {
            id: "t1".into(),
            prompt: "Explore codebase".into(),
            working_dir: PathBuf::from("/tmp/test"),
            timeout_ms: 60_000,
            model: Some("gpt-4".into()),
            max_turns: Some(10),
            profile_hash: Some("abc".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let restored: TaskRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, "t1");
    }

    #[test]
    fn task_result_roundtrip() {
        let result = TaskResult {
            task_id: "t1".into(),
            agent: "codex".into(),
            model: "gpt-4".into(),
            success: true,
            exit_code: 0,
            stdout: "done".into(),
            stderr: String::new(),
            duration_ms: 5000,
            tokens_used: Some(TokenUsage {
                input_tokens: 1000,
                output_tokens: 200,
                cache_read_tokens: 500,
                cache_write_tokens: 100,
            }),
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: TaskResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.agent, "codex");
    }
}
