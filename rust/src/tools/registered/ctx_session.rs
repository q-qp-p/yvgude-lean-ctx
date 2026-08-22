use rmcp::ErrorData;
use rmcp::model::Tool;
use serde_json::{Map, Value, json};
use std::time::Duration;

use crate::server::tool_trait::{McpTool, ToolContext, ToolOutput, get_bool, get_str};
use crate::tool_defs::tool_def;

pub struct CtxSessionTool;

/// Session management is useful, but it must never make the entire MCP server
/// wait behind a long-lived writer. Explicitly retryable operations fail fast.
const SESSION_LOCK_BUDGET: Duration = Duration::from_millis(250);

impl McpTool for CtxSessionTool {
    fn name(&self) -> &'static str {
        "ctx_session"
    }

    fn tool_def(&self) -> Tool {
        tool_def(
            "ctx_session",
            "Session memory. save at session end, load at start, status = snapshot;\n\
             task|finding|decision record progress (value=text).\n\
             ANTIPATTERN: permanent project knowledge → ctx_knowledge.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "status|handoff|load|save|task|finding|decision|list|… (invalid action lists all)"
                    },
                    "value": { "type": "string" },
                    "session_id": { "type": "string", "description": "Omit for latest" }
                },
                "required": ["action"]
            }),
        )
    }

    fn handle(
        &self,
        args: &Map<String, Value>,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ErrorData> {
        let action = get_str(args, "action")
            .ok_or_else(|| ErrorData::invalid_params("action is required", None))?;
        let value = get_str(args, "value");
        let sid = get_str(args, "session_id");
        let format = get_str(args, "format");
        let path = get_str(args, "path");
        let write = get_bool(args, "write").unwrap_or(false);
        let privacy = get_str(args, "privacy");
        let terse = get_bool(args, "terse");

        let tool_calls_handle = ctx
            .tool_calls
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("tool_calls not available", None))?;
        let call_durations: Vec<(String, u64)> = crate::server::bounded_lock::read_for(
            tool_calls_handle,
            "ctx_session tool-call snapshot",
            SESSION_LOCK_BUDGET,
        )
        .map_or_else(Vec::new, |tc| {
            tc.iter().map(|c| (c.tool.clone(), c.duration_ms)).collect()
        });
        let agent_id = ctx
            .agent_id
            .as_ref()
            .and_then(|agent_id| agent_id.try_read().ok().and_then(|id| id.clone()));

        let result = if let Some(result) = crate::tools::ctx_session::handle_without_session(
            &action,
            value.as_deref(),
            sid.as_deref(),
        ) {
            result
        } else {
            let session_handle = ctx
                .session
                .as_ref()
                .ok_or_else(|| ErrorData::internal_error("session not available", None))?;

            if matches!(action.as_str(), "export" | "resume" | "snapshot") {
                let Some(snapshot) = crate::server::bounded_lock::read_for(
                    session_handle,
                    "ctx_session snapshot",
                    SESSION_LOCK_BUDGET,
                )
                .map(|session| session.clone()) else {
                    return Err(ErrorData::internal_error(
                        "session is busy; retry the operation",
                        None,
                    ));
                };
                crate::tools::ctx_session::handle_snapshot_read_action(
                    &snapshot,
                    &action,
                    value.as_deref(),
                    crate::tools::ctx_session::SessionToolOptions {
                        format: format.as_deref(),
                        path: path.as_deref(),
                        write,
                        privacy: privacy.as_deref(),
                        terse,
                        agent_id: agent_id.as_deref(),
                    },
                )
                .expect("snapshot action is explicitly handled")
            } else if matches!(action.as_str(), "status" | "show" | "handoff") {
                let Some(session) = crate::server::bounded_lock::read_for(
                    session_handle,
                    "ctx_session read",
                    SESSION_LOCK_BUDGET,
                ) else {
                    return Err(ErrorData::internal_error(
                        "session is busy; retry the operation",
                        None,
                    ));
                };
                crate::tools::ctx_session::handle_read_only(&session, &action)
                    .expect("read-only action is explicitly handled")
            } else {
                let Some(mut session) = crate::server::bounded_lock::write_for(
                    session_handle,
                    "ctx_session write",
                    SESSION_LOCK_BUDGET,
                ) else {
                    return Err(ErrorData::internal_error(
                        "session is busy; retry the operation",
                        None,
                    ));
                };
                crate::tools::ctx_session::handle(
                    &mut session,
                    &call_durations,
                    &action,
                    value.as_deref(),
                    sid.as_deref(),
                    crate::tools::ctx_session::SessionToolOptions {
                        format: format.as_deref(),
                        path: path.as_deref(),
                        write,
                        privacy: privacy.as_deref(),
                        terse,
                        agent_id: agent_id.as_deref(),
                    },
                )
            }
        };

        Ok(ToolOutput {
            text: result,
            original_tokens: 0,
            saved_tokens: 0,
            mode: Some(action),
            path: None,
            changed: false,
            shell_outcome: None,
            content_blocks: None,
        })
    }
}
