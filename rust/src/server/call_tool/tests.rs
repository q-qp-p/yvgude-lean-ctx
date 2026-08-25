#[allow(clippy::wildcard_imports)]
use super::super::*;
use super::{finalize_call_result, roots_list_failure_is_permanent};

/// Community regression (3.9.19): every documented lossless escape hatch was
/// ignored — triage ran after the compression pipeline, so per-call
/// parameters never reached it. These pins guarantee the bypass contract
/// stays wired at the dispatch chokepoint.
mod triage_bypass_tests {
    use super::super::pipeline::triage_bypass_requested;

    fn args(json: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().expect("test args are an object").clone()
    }

    #[test]
    fn ctx_read_is_always_exempt() {
        // ctx_read delivers file content agents edit against — and its cache
        // marks content as fully delivered BEFORE the pipeline runs, so any
        // post-hoc filtering silently corrupts the dedup contract ("full
        // content already in this conversation" after a stripped delivery).
        assert!(triage_bypass_requested("ctx_read", None));
        assert!(triage_bypass_requested(
            "ctx_read",
            Some(&args(&serde_json::json!({"path": "src/identity.rs"})))
        ));
    }

    #[test]
    fn documented_lossless_escape_hatches_bypass_triage() {
        for a in [
            serde_json::json!({"raw": true}),
            serde_json::json!({"mode": "raw"}),
            serde_json::json!({"mode": "full"}),
            serde_json::json!({"mode": "full-compact"}),
            serde_json::json!({"mode": "lines:32-33"}),
            serde_json::json!({"mode": "anchored:1-10"}),
            serde_json::json!({"mode": "diff"}),
            serde_json::json!({"aggressiveness": 0.0}),
            serde_json::json!({"fresh": true}),
        ] {
            assert!(
                triage_bypass_requested("ctx_shell", Some(&args(&a))),
                "escape hatch must bypass triage: {a}"
            );
        }
    }

    #[test]
    fn plain_calls_remain_subject_to_the_level_gate() {
        assert!(!triage_bypass_requested(
            "ctx_shell",
            Some(&args(&serde_json::json!({"command": "git status"})))
        ));
        assert!(!triage_bypass_requested("ctx_search", None));
    }
}

mod shell_outcome_tests {
    use super::*;
    use crate::server::tool_trait::ShellOutcome;

    fn text_of(result: &CallToolResult) -> String {
        result
            .content
            .first()
            .and_then(|c| c.as_text())
            .map(|t| t.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn success_exit_is_not_an_error() {
        let r = finalize_call_result("ok", Some(ShellOutcome::Exit(0)));
        assert_ne!(r.is_error, Some(true), "exit 0 must not set isError");
        assert!(
            r.structured_content.is_none(),
            "happy path stays token-neutral: no structuredContent on exit 0"
        );
        assert_eq!(text_of(&r), "ok");
    }

    #[test]
    fn nonzero_exit_with_output_is_not_error() {
        // #1090: exit 1 with command output (before [exit:] footer) is NOT
        // a tool error — grep/diff/test exit 1 with results is normal.
        let r = finalize_call_result("boom\n[exit:1]", Some(ShellOutcome::Exit(1)));
        assert_ne!(
            r.is_error,
            Some(true),
            "exit 1 with output must NOT set isError (#1090)"
        );
        // #1127: and it must carry no structuredContent either — clients that
        // prefer it would render `{"exitCode":1}` and drop the output.
        assert!(
            r.structured_content.is_none(),
            "benign exit 1 must not shadow its output with structuredContent (#1127)"
        );
        assert_eq!(text_of(&r), "boom\n[exit:1]", "output text is preserved");
    }

    #[test]
    fn stderr_only_failure_keeps_its_text() {
        // #1127: `ls /nonexistent` writes only to stderr and exits 1. The text
        // block is the sole carrier of the diagnostic, so it must survive.
        let text = "ls: /nonexistent-path-xyz: No such file or directory\n[exit:1]";
        let r = finalize_call_result(text, Some(ShellOutcome::Exit(1)));
        assert_ne!(r.is_error, Some(true));
        assert!(r.structured_content.is_none());
        assert_eq!(text_of(&r), text);
    }

    #[test]
    fn timeout_with_partial_output_keeps_its_text() {
        // #1086/#1127: same shape for exit 124 — partial output is a success
        // result, so structuredContent must not displace it.
        let text = "line one\nERROR: command timed out after 5000ms";
        let r = finalize_call_result(text, Some(ShellOutcome::Exit(124)));
        assert_ne!(r.is_error, Some(true));
        assert!(r.structured_content.is_none());
        assert_eq!(text_of(&r), text);
    }

    #[test]
    fn exit_1_without_output_is_error() {
        // Exit 1 with only the [exit:] footer (no command output) IS an error.
        let r = finalize_call_result("[exit:1]", Some(ShellOutcome::Exit(1)));
        assert_eq!(
            r.is_error,
            Some(true),
            "exit 1 with no command output must set isError"
        );
    }

    #[test]
    fn exit_2_is_always_error() {
        let r = finalize_call_result("error output", Some(ShellOutcome::Exit(2)));
        assert_eq!(
            r.is_error,
            Some(true),
            "exit >= 2 must always set isError (#389)"
        );
        assert_eq!(
            r.structured_content,
            Some(serde_json::json!({ "exitCode": 2 })),
            "guards must be able to read exitCode without text parsing"
        );
    }

    #[test]
    fn negative_exit_codes_are_reported() {
        // Signal terminations are mapped to negative/128+n codes by execute();
        // whatever the value, non-zero must surface as an error.
        let r = finalize_call_result("killed", Some(ShellOutcome::Exit(-1)));
        assert_eq!(r.is_error, Some(true));
        assert_eq!(
            r.structured_content,
            Some(serde_json::json!({ "exitCode": -1 }))
        );
    }

    #[test]
    fn blocked_command_sets_is_error_and_blocked_marker() {
        let r = finalize_call_result("[BLOCKED] nope", Some(ShellOutcome::Blocked));
        assert_eq!(
            r.is_error,
            Some(true),
            "blocked commands never ran — that is a failure"
        );
        assert_eq!(
            r.structured_content,
            Some(serde_json::json!({ "blocked": true }))
        );
    }

    #[test]
    fn non_shell_tools_are_unaffected() {
        let r = finalize_call_result("file contents", None);
        assert_ne!(r.is_error, Some(true));
        assert!(r.structured_content.is_none());
    }
}

#[cfg(test)]
mod roots_retry_tests {
    use super::roots_list_failure_is_permanent;

    /// Cursor's pattern (#699): roots capability declared, `roots/list`
    /// answered with `-32601` — retrying is pointless and must stop.
    #[test]
    fn method_not_found_is_permanent() {
        let err = rmcp::ServiceError::McpError(rmcp::model::ErrorData::new(
            rmcp::model::ErrorCode::METHOD_NOT_FOUND,
            "Method not found",
            None,
        ));
        assert!(roots_list_failure_is_permanent(&err));
    }

    /// The VS Code multi-window pattern (GH #694): the second window's client
    /// is still starting up, `roots/list` times out or the transport hiccups —
    /// these must stay retryable so root detection recovers.
    #[test]
    fn timeouts_and_other_mcp_errors_are_transient() {
        let timeout = rmcp::ServiceError::Timeout {
            timeout: std::time::Duration::from_secs(5),
        };
        assert!(!roots_list_failure_is_permanent(&timeout));

        let internal = rmcp::ServiceError::McpError(rmcp::model::ErrorData::new(
            rmcp::model::ErrorCode::INTERNAL_ERROR,
            "boom",
            None,
        ));
        assert!(!roots_list_failure_is_permanent(&internal));
    }
}

#[cfg(test)]
mod response_cache_tests {
    use super::super::guarded::{cache_call_result, cached_call_result, response_cache_key};
    use crate::core::ocla::response_cache::{CachedResponse, ResponseCache};
    use rmcp::model::{CallToolResult, ContentBlock};
    use serde_json::{Map, json};
    use std::time::{Duration, Instant};

    fn arguments() -> Map<String, serde_json::Value> {
        Map::from_iter([("path".to_owned(), json!("src/lib.rs"))])
    }

    fn text_of(result: &CallToolResult) -> &str {
        result.content[0]
            .as_text()
            .expect("cached result must be text")
            .text
            .as_str()
    }

    #[test]
    fn only_deterministic_tools_receive_cache_keys() {
        for tool in ["ctx_search", "ctx_tree", "ctx_glob"] {
            assert!(response_cache_key(tool, Some(&arguments()), "/project").is_some());
        }
        for tool in ["ctx_shell", "ctx_session", "ctx_knowledge", "ctx_call"] {
            assert!(response_cache_key(tool, Some(&arguments()), "/project").is_none());
        }
    }

    #[test]
    fn cache_keys_include_arguments_and_project_scope() {
        let base = response_cache_key("ctx_search", Some(&arguments()), "/project").unwrap();
        let other_root = response_cache_key("ctx_search", Some(&arguments()), "/other").unwrap();
        let other_args = Map::from_iter([("path".to_owned(), json!("src/main.rs"))]);
        let other_path = response_cache_key("ctx_search", Some(&other_args), "/project").unwrap();

        assert_ne!(base, other_root);
        assert_ne!(base, other_path);
        assert_eq!(
            base,
            response_cache_key("ctx_search", Some(&arguments()), "/project").unwrap()
        );
    }

    #[test]
    fn successful_text_response_round_trips_through_cache() {
        let cache = ResponseCache::new(8, Duration::from_mins(1));
        let key = response_cache_key("ctx_search", Some(&arguments()), "/project").unwrap();
        let mut result = CallToolResult::success(vec![ContentBlock::text("search result")]);
        result.structured_content = Some(json!({"matches": 1}));

        cache_call_result(&cache, key.clone(), &result);

        let cached = cached_call_result(&cache, &key).expect("response should be cached");
        assert_eq!(text_of(&cached), "search result");
        assert_eq!(cached.structured_content, Some(json!({"matches": 1})));
        assert_ne!(cached.is_error, Some(true));
    }

    #[test]
    fn expired_response_is_not_returned() {
        let cache = ResponseCache::new(8, Duration::from_secs(1));
        let key = response_cache_key("ctx_tree", Some(&arguments()), "/project").unwrap();
        cache.put(
            key.clone(),
            CachedResponse {
                body: b"stale".to_vec(),
                status: 200,
                tokens: 1,
                created_at: Instant::now().checked_sub(Duration::from_secs(2)).unwrap(),
                ttl: Duration::from_secs(1),
            },
        );

        assert!(cached_call_result(&cache, &key).is_none());
    }
}
