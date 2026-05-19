use crate::core::context_ledger::ContextLedger;

pub fn cmd_ledger(args: &[String]) {
    let action = args.first().map_or("status", String::as_str);

    match action {
        "status" => {
            #[cfg(unix)]
            if let Some(out) = crate::daemon_client::try_daemon_tool_call_blocking_text(
                "ctx_ledger",
                Some(serde_json::json!({ "action": "status" })),
            ) {
                println!("{out}");
                return;
            }
            let ledger = ContextLedger::load();
            let pressure = ledger.pressure();
            println!(
                "Context pressure: {:.0}% ({}/{} tokens)",
                pressure.utilization * 100.0,
                ledger.total_tokens_sent,
                ledger.window_size,
            );
            println!("Entries: {}", ledger.entries.len());
            println!("Recommendation: {:?}", pressure.recommendation);
            let top = ledger.files_by_token_cost();
            if !top.is_empty() {
                println!("Top files by cost:");
                for (path, tokens) in top.iter().take(5) {
                    println!("  {path} ({tokens} tok)");
                }
            }
        }

        "reset" => {
            #[cfg(unix)]
            if let Some(out) = crate::daemon_client::try_daemon_tool_call_blocking_text(
                "ctx_ledger",
                Some(serde_json::json!({ "action": "reset" })),
            ) {
                println!("{out}");
                return;
            }
            let mut ledger = ContextLedger::load();
            let prev_entries = ledger.entries.len();
            let prev_tokens = ledger.total_tokens_sent;
            ledger.reset();
            ledger.save();
            println!(
                "Ledger reset. Removed {prev_entries} entries, freed {prev_tokens} tracked tokens. Pressure: 0%."
            );
        }

        "evict" => {
            let targets: Vec<&str> = args[1..].iter().map(String::as_str).collect();
            if targets.is_empty() {
                eprintln!("Usage: lean-ctx ledger evict <file1> [file2...]");
                std::process::exit(1);
            }

            #[cfg(unix)]
            {
                let targets_joined = targets.join(", ");
                if let Some(out) = crate::daemon_client::try_daemon_tool_call_blocking_text(
                    "ctx_ledger",
                    Some(serde_json::json!({ "action": "evict", "targets": targets_joined })),
                ) {
                    println!("{out}");
                    return;
                }
            }

            let mut ledger = ContextLedger::load();
            let removed = ledger.evict_paths(&targets);
            ledger.save();
            let pressure = ledger.pressure();
            println!(
                "Evicted {removed}/{} target(s). Pressure now: {:.0}%.",
                targets.len(),
                pressure.utilization * 100.0,
            );
        }

        "prune" => {
            let mut ledger = ContextLedger::load();
            let pruned = ledger.prune();
            ledger.save();
            let pressure = ledger.pressure();
            println!(
                "Pruned {pruned} entries. Remaining: {}. Pressure: {:.0}%.",
                ledger.entries.len(),
                pressure.utilization * 100.0,
            );
        }

        _ => {
            eprintln!("Usage: lean-ctx ledger <status|reset|evict|prune> [args...]");
            std::process::exit(1);
        }
    }
}
