use crate::server::background_shell::JobState;
use crate::server::tool_trait::{
    BackgroundDisplay, BackgroundJobState, BackgroundLookupError, BackgroundShellOutcome,
    ShellOutcome,
};

/// Render a `background_action` result.
///
/// #1246: a caller-requested cancel is a success, not a tool failure. The
/// process's own SIGINT exit (130) used to be reported as the tool's exit code,
/// which tripped the client's failure hook and told the agent to fix something
/// it had deliberately done. A cancel therefore never reports a non-zero exit,
/// and is idempotent: cancelling an already-cancelled, already-finished or
/// already-pruned job is equally benign. The first cancel also gets its own
/// wording so it cannot be mistaken for a status poll that did nothing.
pub(super) fn format_background_state(
    id: &str,
    is_cancel: bool,
    state: Option<JobState>,
) -> (String, ShellOutcome) {
    let Some(state) = state else {
        return if is_cancel {
            (
                format!("[background:{id} not found — already finished or cancelled]"),
                ShellOutcome::Exit(0),
            )
        } else {
            (
                format!("[background:{id} not found or expired]"),
                ShellOutcome::BackgroundLookupError(BackgroundLookupError {
                    job_id: id.to_string(),
                    code: "background_job_not_found_or_expired".to_string(),
                    reason: "job not found or retained terminal verdict expired".to_string(),
                }),
            )
        };
    };
    match state {
        JobState::Running { output } => {
            // #1217: show the captured-so-far output so a poll of a
            // long-running job reflects progress instead of a bare
            // "running" with no signal of whether it is advancing.
            let output = redact_shell_output_secrets(&output);
            let head = if is_cancel {
                format!(
                    "[background:{id} cancel requested — job is stopping; poll status for the final output]"
                )
            } else {
                format!("[background:{id} running]")
            };
            (
                output.clone(),
                ShellOutcome::Background(BackgroundShellOutcome {
                    state: BackgroundJobState::Running,
                    exit_code: None,
                    job_id: id.to_string(),
                    archive_id: None,
                    archive_truncated: None,
                    captured_chars: None,
                    archived_chars: None,
                    summary: summarize_background_output(&output),
                    is_error: false,
                    display: Some(BackgroundDisplay {
                        header: head,
                        footer: None,
                    }),
                }),
            )
        }
        JobState::Completed { output, exit_code } => {
            let output = redact_shell_output_secrets(&output);
            let state = if exit_code == 0 {
                BackgroundJobState::Completed
            } else {
                BackgroundJobState::Failed
            };
            let head = format!("[background:{id} {}, exit {exit_code}]", state.as_str());
            let footer = (exit_code != 0).then(|| format!("[exit:{exit_code}]"));
            (
                output.clone(),
                ShellOutcome::Background(BackgroundShellOutcome {
                    state,
                    exit_code: Some(exit_code),
                    job_id: id.to_string(),
                    archive_id: None,
                    archive_truncated: None,
                    captured_chars: None,
                    archived_chars: None,
                    summary: summarize_background_output(&output),
                    is_error: !is_cancel && exit_code != 0,
                    display: Some(BackgroundDisplay {
                        header: head,
                        footer,
                    }),
                }),
            )
        }
        JobState::Cancelled { output } => {
            let output = redact_shell_output_secrets(&output);
            (
                output.clone(),
                ShellOutcome::Background(BackgroundShellOutcome {
                    state: BackgroundJobState::Cancelled,
                    exit_code: Some(130),
                    job_id: id.to_string(),
                    archive_id: None,
                    archive_truncated: None,
                    captured_chars: None,
                    archived_chars: None,
                    summary: summarize_background_output(&output),
                    is_error: false,
                    display: Some(BackgroundDisplay {
                        header: format!("[background:{id} cancelled, exit 130]"),
                        footer: Some(format!("[cancelled: {id}, exit 130]")),
                    }),
                }),
            )
        }
    }
}

fn summarize_background_output(output: &str) -> String {
    let chars = output.chars().count();
    let lines = output.lines().count();
    let trimmed = output.trim();
    if trimmed.is_empty() {
        "no output".to_string()
    } else if chars <= 512 {
        trimmed.to_string()
    } else {
        format!("{chars} chars, {lines} lines")
    }
}

pub(super) fn redact_shell_output_secrets(output: &str) -> String {
    let output = crate::core::redaction::redact_text_if_enabled(output);
    let cfg = crate::core::config::Config::load();
    if !cfg.secret_detection.enabled {
        return output;
    }
    let (redacted, matches) =
        crate::core::secret_detection::scan_and_redact(&output, &cfg.secret_detection);
    if !matches.is_empty() {
        let names: Vec<&str> = matches.iter().map(|m| m.pattern_name).collect();
        tracing::warn!(
            "[SHELL SECRET REDACTION] {} secret(s) redacted from shell output: {}",
            matches.len(),
            names.join(", ")
        );
    }
    redacted
}
