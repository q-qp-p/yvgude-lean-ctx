use rmcp::model::Tool;
use rmcp::ErrorData;
use serde_json::{json, Map, Value};

use crate::core::context_field::ContextItemId;
use crate::core::context_overlay::{
    ContextOverlay, OverlayAuthor, OverlayOp, OverlayScope, OverlayStore,
};
use crate::server::tool_trait::{get_str, McpTool, ToolContext, ToolOutput};
use crate::tool_defs::tool_def;

pub struct CtxLedgerTool;

impl McpTool for CtxLedgerTool {
    fn name(&self) -> &'static str {
        "ctx_ledger"
    }

    fn tool_def(&self) -> Tool {
        tool_def(
            "ctx_ledger",
            "Context ledger ops: status|reset|evict. Manages persistent context pressure.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "reset", "evict"],
                        "description": "Ledger operation: status (show pressure), reset (clear all), evict (remove specific files)"
                    },
                    "targets": {
                        "type": "string",
                        "description": "Comma-separated file paths to evict (required for 'evict' action)"
                    }
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

        let ledger_arc = ctx
            .ledger
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("ledger not available", None))?;

        let result = match action.as_str() {
            "status" => {
                let ledger = tokio::task::block_in_place(|| ledger_arc.blocking_read());
                let pressure = ledger.pressure();
                let top_files: Vec<String> = ledger
                    .files_by_token_cost()
                    .iter()
                    .take(5)
                    .map(|(path, tokens)| {
                        format!(
                            "  {} ({} tok)",
                            crate::core::protocol::shorten_path(path),
                            tokens
                        )
                    })
                    .collect();

                let mut lines = vec![
                    format!(
                        "Context pressure: {:.0}% ({}/{} tokens)",
                        pressure.utilization * 100.0,
                        ledger.total_tokens_sent,
                        ledger.window_size,
                    ),
                    format!("Entries: {}", ledger.entries.len()),
                    format!("Recommendation: {:?}", pressure.recommendation),
                ];
                if !top_files.is_empty() {
                    lines.push("Top files by cost:".to_string());
                    lines.extend(top_files);
                }
                lines.join("\n")
            }

            "reset" => {
                let mut ledger = tokio::task::block_in_place(|| ledger_arc.blocking_write());
                let prev_entries = ledger.entries.len();
                let prev_tokens = ledger.total_tokens_sent;
                ledger.reset();
                ledger.save();
                format!(
                    "Ledger reset. Removed {prev_entries} entries, freed {prev_tokens} tracked tokens. Pressure: 0%."
                )
            }

            "evict" => {
                let targets_str = get_str(args, "targets").ok_or_else(|| {
                    ErrorData::invalid_params(
                        "targets is required for evict action (comma-separated paths)",
                        None,
                    )
                })?;

                let targets: Vec<&str> = targets_str.split(',').map(str::trim).collect();
                if targets.is_empty() {
                    return Ok(ToolOutput::simple(
                        "No targets specified for eviction.".to_string(),
                    ));
                }

                let mut ledger = tokio::task::block_in_place(|| ledger_arc.blocking_write());
                let removed = ledger.evict_paths(&targets);

                // Add exclude overlays to prevent re-accumulation
                let root = if ctx.project_root.is_empty() {
                    "."
                } else {
                    &ctx.project_root
                };
                let mut overlays = OverlayStore::load_project(&std::path::PathBuf::from(root));
                for target in &targets {
                    let item_id = ContextItemId::from_file(target);
                    let overlay = ContextOverlay::new(
                        item_id,
                        OverlayOp::Exclude {
                            reason: "evicted by ctx_ledger".into(),
                        },
                        OverlayScope::Session,
                        String::new(),
                        OverlayAuthor::Policy("ctx_ledger_evict".into()),
                    );
                    overlays.add(overlay);
                }
                let _ = overlays.save_project(&std::path::PathBuf::from(root));

                ledger.save();

                let pressure = ledger.pressure();
                format!(
                    "Evicted {removed}/{} target(s). Pressure now: {:.0}%. Files excluded from re-accumulation until session reset.",
                    targets.len(),
                    pressure.utilization * 100.0,
                )
            }

            _ => "Unknown action. Use: status, reset, evict".to_string(),
        };

        let changed = action != "status";
        Ok(ToolOutput {
            text: result,
            original_tokens: 0,
            saved_tokens: 0,
            mode: Some(action),
            path: None,
            changed,
        })
    }
}
