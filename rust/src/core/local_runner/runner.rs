use std::path::PathBuf;

use anyhow::Result;

use crate::core::agent_connector::traits::{AgentConnector, TaskRequest, TaskResult};
use crate::core::benchmark_spec::types::{
    BenchmarkOutcome, BenchmarkResult, BenchmarkSpecV1, BenchmarkSummary, BenchmarkTask, TaskKind,
};

const DEFAULT_TIMEOUT_MS: u64 = 300_000;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct RunConfig {
    pub agent_name: String,
    pub profile_name: String,
    pub suite_name: Option<String>,
    pub timeout_override_ms: Option<u64>,
    pub working_dir: PathBuf,
    pub repeats: u32,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            agent_name: String::new(),
            profile_name: "coder".into(),
            suite_name: None,
            timeout_override_ms: None,
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            repeats: 1,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum RunProgress {
    Starting,
    DetectingAgents,
    LoadingProfile,
    RunningTask {
        task_index: usize,
        total: usize,
        task_id: String,
    },
    TaskComplete {
        task_id: String,
        passed: bool,
    },
    AllComplete,
}

#[allow(dead_code)]
pub(crate) struct LocalRunner {
    config: RunConfig,
    connector: Box<dyn AgentConnector>,
}

#[allow(dead_code)]
impl LocalRunner {
    pub(crate) fn new(config: RunConfig, connector: Box<dyn AgentConnector>) -> Self {
        Self { config, connector }
    }

    pub(crate) fn run(&self, spec: &BenchmarkSpecV1) -> Result<BenchmarkResult> {
        self.run_with_progress(spec, |_| {})
    }

    pub(crate) fn run_with_progress<F>(
        &self,
        spec: &BenchmarkSpecV1,
        on_progress: F,
    ) -> Result<BenchmarkResult>
    where
        F: Fn(RunProgress),
    {
        on_progress(RunProgress::Starting);
        let total = spec.suite.tasks.len();
        let mut outcomes = Vec::with_capacity(total * self.config.repeats as usize);

        for repeat in 0..self.config.repeats {
            for (idx, task) in spec.suite.tasks.iter().enumerate() {
                on_progress(RunProgress::RunningTask {
                    task_index: idx + (repeat as usize * total),
                    total: total * self.config.repeats as usize,
                    task_id: task.id.clone(),
                });
                let request = self.task_request(spec, task);
                let result = self.connector.execute(&request)?;
                let passed = result.success;
                let outcome = outcome_from_task_result(task, &result);
                on_progress(RunProgress::TaskComplete {
                    task_id: task.id.clone(),
                    passed,
                });
                outcomes.push(outcome);
            }
        }

        let summary = BenchmarkSummary::from_outcomes(&outcomes, spec.configuration.quality_floor);
        on_progress(RunProgress::AllComplete);

        Ok(BenchmarkResult {
            spec_id: spec.id.clone(),
            spec_version: spec.version.clone(),
            profile_hash: spec.configuration.profile_hash.clone().unwrap_or_default(),
            agent: self.connector.name().to_owned(),
            model: spec
                .configuration
                .model
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
            runtime_version: spec.configuration.runtime_version.clone(),
            outcomes,
            summary,
            completed_at: current_unix_timestamp(),
        })
    }

    fn task_request(&self, spec: &BenchmarkSpecV1, task: &BenchmarkTask) -> TaskRequest {
        TaskRequest {
            id: task.id.clone(),
            prompt: task_to_prompt(task),
            working_dir: self.config.working_dir.clone(),
            timeout_ms: self
                .config
                .timeout_override_ms
                .or(task.timeout_ms)
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            model: spec.configuration.model.clone(),
            max_turns: None,
            profile_hash: spec.configuration.profile_hash.clone(),
        }
    }
}

#[allow(dead_code)]
pub(crate) fn task_to_prompt(task: &BenchmarkTask) -> String {
    let kind_instruction = match task.kind {
        TaskKind::Explore => "Explore and understand the repository structure.",
        TaskKind::LocateRegression => "Find the source of a regression in the codebase.",
        TaskKind::FixBug => "Diagnose and fix the reported bug.",
        TaskKind::RunTests => "Run the project's test suite and report results.",
        TaskKind::ExplainArchitecture => "Produce a clear architectural summary.",
        TaskKind::Custom => "Complete the following task as described.",
    };
    format!("{kind_instruction}\n\n{}", task.description)
}

fn outcome_from_task_result(task: &BenchmarkTask, result: &TaskResult) -> BenchmarkOutcome {
    let tokens_in = result.tokens_used.as_ref().map_or(0, |t| t.input_tokens);
    let tokens_out = result.tokens_used.as_ref().map_or(0, |t| t.output_tokens);
    let cost = (tokens_in as f64 * 3.0 + tokens_out as f64 * 15.0) / 1_000_000.0;
    BenchmarkOutcome {
        task_id: task.id.clone(),
        passed: result.success,
        cost_usd: cost,
        quality_score: if result.success { 1.0 } else { 0.0 },
        latency_ms: result.duration_ms,
        tokens_input: tokens_in,
        tokens_output: tokens_out,
        error: if result.stderr.is_empty() {
            None
        } else {
            Some(result.stderr.clone())
        },
    }
}

fn current_unix_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_connector::traits::{AgentInfo, TokenUsage};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockConnector {
        should_succeed: bool,
    }
    impl MockConnector {
        fn new(should_succeed: bool) -> Self {
            Self { should_succeed }
        }
    }

    impl AgentConnector for MockConnector {
        fn info(&self) -> AgentInfo {
            AgentInfo {
                name: "mock".into(),
                version: Some("1.0.0".into()),
                path: PathBuf::from("/usr/bin/mock"),
                capabilities: vec!["execute".into()],
                available: true,
            }
        }
        fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
        fn execute(&self, request: &TaskRequest) -> Result<TaskResult> {
            Ok(TaskResult {
                task_id: request.id.clone(),
                agent: "mock".into(),
                model: "test-model".into(),
                success: self.should_succeed,
                exit_code: if self.should_succeed { 0 } else { 1 },
                stdout: "task output".into(),
                stderr: String::new(),
                duration_ms: 1000,
                tokens_used: Some(TokenUsage {
                    input_tokens: 500,
                    output_tokens: 200,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                }),
            })
        }
        fn name(&self) -> &'static str {
            "mock"
        }
    }

    fn test_spec() -> BenchmarkSpecV1 {
        use crate::core::benchmark_spec::types::{
            BenchmarkConfiguration, BenchmarkKind, BenchmarkSuite,
        };
        BenchmarkSpecV1 {
            id: "test-spec".into(),
            version: "1.0.0".into(),
            name: "Test".into(),
            description: "Test benchmark".into(),
            suite: BenchmarkSuite {
                kind: BenchmarkKind::TaskScore,
                tasks: vec![
                    BenchmarkTask {
                        id: "t1".into(),
                        name: "Task 1".into(),
                        description: "Do task 1".into(),
                        kind: TaskKind::Explore,
                        timeout_ms: Some(60_000),
                    },
                    BenchmarkTask {
                        id: "t2".into(),
                        name: "Task 2".into(),
                        description: "Do task 2".into(),
                        kind: TaskKind::FixBug,
                        timeout_ms: Some(120_000),
                    },
                ],
            },
            configuration: BenchmarkConfiguration {
                profile_hash: Some("abc123".into()),
                agent: None,
                model: None,
                runtime_version: "test".into(),
                repeats: 1,
                quality_floor: 0.95,
            },
            created_at: "0".into(),
        }
    }

    #[test]
    fn new_runner_sets_config() {
        let runner = LocalRunner::new(RunConfig::default(), Box::new(MockConnector::new(true)));
        assert_eq!(runner.config.profile_name, "coder");
    }

    #[test]
    fn run_with_mock_connector() {
        let runner = LocalRunner::new(RunConfig::default(), Box::new(MockConnector::new(true)));
        let result = runner.run(&test_spec()).unwrap();
        assert_eq!(result.outcomes.len(), 2);
        assert!(result.outcomes.iter().all(|o| o.passed));
        assert_eq!(result.summary.passed_tasks, 2);
    }

    #[test]
    fn progress_callback_fires() {
        let counter = AtomicUsize::new(0);
        let runner = LocalRunner::new(RunConfig::default(), Box::new(MockConnector::new(true)));
        runner
            .run_with_progress(&test_spec(), |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 6);
    }

    #[test]
    fn task_to_prompt_covers_all_kinds() {
        for kind in &[
            TaskKind::Explore,
            TaskKind::LocateRegression,
            TaskKind::FixBug,
            TaskKind::RunTests,
            TaskKind::ExplainArchitecture,
            TaskKind::Custom,
        ] {
            let task = BenchmarkTask {
                id: "test".into(),
                name: "Test".into(),
                description: "Test task".into(),
                kind: kind.clone(),
                timeout_ms: None,
            };
            let prompt = task_to_prompt(&task);
            assert!(!prompt.is_empty());
            assert!(prompt.contains(&task.description));
        }
    }
}
