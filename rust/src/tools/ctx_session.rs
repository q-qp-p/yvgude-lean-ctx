use crate::core::session::SessionState;

#[derive(Clone, Copy, Debug)]
pub struct SessionToolOptions<'a> {
    pub format: Option<&'a str>,
    pub path: Option<&'a str>,
    pub write: bool,
    pub privacy: Option<&'a str>,
    /// For `action=configure`: set terse output mode when `Some`.
    pub terse: Option<bool>,
}

pub fn handle(
    session: &mut SessionState,
    tool_calls: &[(String, u64)],
    action: &str,
    value: Option<&str>,
    session_id: Option<&str>,
    opts: SessionToolOptions<'_>,
) -> String {
    match action {
        "status" => session.format_compact(),

        "load" => {
            let loaded = if let Some(id) = session_id {
                SessionState::load_by_id(id)
            } else {
                SessionState::load_latest()
            };

            if let Some(prev) = loaded {
                let summary = prev.format_compact();
                *session = prev;
                format!("Session loaded.\n{summary}")
            } else {
                let id_str = session_id.unwrap_or("latest");
                format!("No session found (id: {id_str}). Starting fresh.")
            }
        }

        "save" => {
            match session.save() {
                Ok(()) => format!("Session {} saved (v{}).", session.id, session.version),
                Err(e) => format!("Save failed: {e}"),
            }
        }

        "export" => {
            let requested_privacy = crate::core::ccp_session_bundle::BundlePrivacyV1::parse(opts.privacy);
            if requested_privacy == crate::core::ccp_session_bundle::BundlePrivacyV1::Full
                && crate::core::roles::active_role_name() != "admin"
            {
                return "ERROR: privacy=full requires role 'admin'.".to_string();
            }

            let bundle =
                crate::core::ccp_session_bundle::build_bundle_v1(session, requested_privacy);
            let json = match crate::core::ccp_session_bundle::serialize_bundle_v1_pretty(&bundle) {
                Ok(s) => s,
                Err(e) => return e,
            };

            let format = opts
                .format
                .unwrap_or(if opts.write { "summary" } else { "json" });
            let root = session.project_root.clone().unwrap_or_else(|| {
                std::env::current_dir().map_or_else(
                    |_| ".".to_string(),
                    |p| p.to_string_lossy().to_string(),
                )
            });
            let root_path = std::path::PathBuf::from(&root);

            let mut written: Option<String> = None;
            if opts.write || opts.path.is_some() {
                let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
                let candidate = if let Some(p) = opts.path.or(value) {
                    let p = std::path::PathBuf::from(p);
                    if p.is_absolute() { p } else { root_path.join(p) }
                } else {
                    root_path
                        .join(".lean-ctx")
                        .join("session_bundles")
                        .join(format!("ccp-session-bundle-v1_{}_{}.json", bundle.session.id, ts))
                };

                let jailed = match crate::core::io_boundary::jail_and_check_path(
                    "ctx_session.export",
                    candidate.as_path(),
                    root_path.as_path(),
                ) {
                    Ok((p, _warning)) => p,
                    Err(e) => return e,
                };

                if let Err(e) = crate::core::ccp_session_bundle::write_bundle_v1(&jailed, &json) {
                    return format!("Export write failed: {e}");
                }
                written = Some(jailed.to_string_lossy().to_string());
            }

            match format {
                "summary" => {
                    let mut out = format!(
                        "CCP session bundle exported (v{}).\n\
schema_version: {}\n\
session_id: {}\n\
bytes: {}\n",
                        bundle.session.version,
                        bundle.schema_version,
                        bundle.session.id,
                        json.len()
                    );
                    if let Some(p) = written {
                        out.push_str(&format!("path: {p}\n"));
                    }
                    if let Some(h) = bundle.project.project_root_hash {
                        out.push_str(&format!("project_root_hash: {h}\n"));
                    }
                    if let Some(h) = bundle.project.project_identity_hash {
                        out.push_str(&format!("project_identity_hash: {h}\n"));
                    }
                    out
                }
                _ => {
                    if let Some(p) = written {
                        format!("{json}\n\npath: {p}")
                    } else {
                        json
                    }
                }
            }
        }

        "import" => {
            let root = session.project_root.clone().unwrap_or_else(|| {
                std::env::current_dir().map_or_else(
                    |_| ".".to_string(),
                    |p| p.to_string_lossy().to_string(),
                )
            });
            let root_path = std::path::PathBuf::from(&root);

            let Some(p) = opts.path.or(value) else {
                return "ERROR: path is required for action=import".to_string();
            };

            let candidate = {
                let p = std::path::PathBuf::from(p);
                if p.is_absolute() { p } else { root_path.join(p) }
            };
            let jailed = match crate::core::io_boundary::jail_and_check_path(
                "ctx_session.import",
                candidate.as_path(),
                root_path.as_path(),
            ) {
                Ok((p, _warning)) => p,
                Err(e) => return e,
            };

            let bundle = match crate::core::ccp_session_bundle::read_bundle_v1(&jailed) {
                Ok(b) => b,
                Err(e) => return format!("Import failed: {e}"),
            };

            // Replayability hint: compare project identity hashes (best-effort).
            let current_root_hash = crate::core::project_hash::hash_project_root(&root);
            let current_identity_hash = crate::core::project_hash::project_identity(&root)
                .as_deref()
                .map(|s| {
                    use md5::{Digest, Md5};
                    let mut h = Md5::new();
                    h.update(s.as_bytes());
                    format!("{:x}", h.finalize())
                });

            let mut warning: Option<String> = None;
            if let Some(ref exported) = bundle.project.project_root_hash {
                if exported != &current_root_hash {
                    warning = Some("WARNING: project_root_hash mismatch (importing into different project root).".to_string());
                }
            }
            if let (Some(exported), Some(current)) =
                (bundle.project.project_identity_hash.as_ref(), current_identity_hash.as_ref())
            {
                if exported != current {
                    warning = Some("WARNING: project_identity_hash mismatch (importing into different project identity).".to_string());
                }
            }

            let report = crate::core::ccp_session_bundle::import_bundle_v1_into_session(
                session,
                &bundle,
                Some(&root),
            );
            let _ = session.save();

            let mut out = format!(
                "CCP session bundle imported.\n\
session_id: {}\n\
version: {}\n\
files_touched: {}\n\
stale_files: {}\n",
                report.session_id, report.version, report.files_touched, report.stale_files
            );
            if let Some(w) = warning {
                out.push_str(&format!("{w}\n"));
            }
            out
        }

        "task" => {
            let desc = value.unwrap_or("(no description)");
            session.set_task(desc, None);
            format!("Task set: {desc}")
        }

        "finding" => {
            let summary = value.unwrap_or("(no summary)");
            let (file, line, text) = parse_finding_value(summary);
            session.add_finding(file.as_deref(), line, text);
            format!("Finding added: {summary}")
        }

        "decision" => {
            let desc = value.unwrap_or("(no description)");
            session.add_decision(desc, None);
            format!("Decision recorded: {desc}")
        }

        "reset" => {
            let _ = session.save();
            let old_id = session.id.clone();
            *session = SessionState::new();
            crate::core::budget_tracker::BudgetTracker::global().reset();
            // Clear the persistent context ledger so pressure resets to 0%
            let mut ledger = crate::core::context_ledger::ContextLedger::load();
            ledger.reset();
            ledger.save();
            if let Ok(data_dir) = crate::core::data_dir::lean_ctx_data_dir() {
                let radar_path = data_dir.join("context_radar.jsonl");
                let prev = data_dir.join("context_radar.prev.jsonl");
                let _ = std::fs::rename(&radar_path, &prev);
            }
            format!(
                "Session reset. Previous: {old_id}. New: {}. Ledger cleared (0% pressure).",
                session.id
            )
        }

        "list" => {
            let sessions = SessionState::list_sessions();
            if sessions.is_empty() {
                return "No sessions found.".to_string();
            }
            let mut lines = vec![format!("Sessions ({}):", sessions.len())];
            for s in sessions.iter().take(10) {
                let task = s.task.as_deref().unwrap_or("(no task)");
                let task_short: String = task.chars().take(40).collect();
                lines.push(format!(
                    "  {} v{} | {} calls | {} tok | {}",
                    s.id, s.version, s.tool_calls, s.tokens_saved, task_short
                ));
            }
            if sessions.len() > 10 {
                lines.push(format!("  ... +{} more", sessions.len() - 10));
            }
            lines.join("\n")
        }

        "cleanup" => {
            let removed = SessionState::cleanup_old_sessions(7);
            format!("Cleaned up {removed} old session(s) (>7 days).")
        }

        "configure" => match opts.terse {
            Some(enabled) => {
                session.terse_mode = enabled;
                session.increment();
                format!("Session configured: terse_mode={enabled}")
            }
            None => format!("Session config: terse_mode={}", session.terse_mode),
        },

        "snapshot" => match session.save_compaction_snapshot() {
            Ok(snapshot) => {
                format!(
                    "Compaction snapshot saved ({} bytes).\n{snapshot}",
                    snapshot.len()
                )
            }
            Err(e) => format!("Snapshot failed: {e}"),
        },

        "restore" => {
            let snapshot = if let Some(id) = session_id {
                SessionState::load_compaction_snapshot(id)
            } else {
                SessionState::load_latest_snapshot()
            };
            match snapshot {
                Some(s) => format!("Session restored from compaction snapshot:\n{s}"),
                None => "No compaction snapshot found. Session continues fresh.".to_string(),
            }
        }

        "resume" => session.build_resume_block(),

        "profile" => {
            use crate::core::profiles;
            if let Some(name) = value {
                if let Ok(p) = profiles::set_active_profile(name) {
                    format!(
                        "Profile switched to '{name}'.\n\
                         Read mode: {}, Budget: {} tokens, CRP: {}, Density: {}",
                        p.read.default_mode_effective(),
                        p.budget.max_context_tokens_effective(),
                        p.compression.crp_mode_effective(),
                        p.compression.output_density_effective(),
                    )
                } else {
                    let available: Vec<String> =
                        profiles::list_profiles().iter().map(|p| p.name.clone()).collect();
                    format!(
                        "Profile '{name}' not found. Available: {}",
                        available.join(", ")
                    )
                }
            } else {
                let name = profiles::active_profile_name();
                let p = profiles::active_profile();
                let list = profiles::list_profiles();
                let mut out = format!(
                    "Active profile: {name}\n\
                     Read: {}, Budget: {} tok, CRP: {}, Density: {}\n\n\
                     Available profiles:",
                    p.read.default_mode_effective(),
                    p.budget.max_context_tokens_effective(),
                    p.compression.crp_mode_effective(),
                    p.compression.output_density_effective(),
                );
                for info in &list {
                    let marker = if info.name == name { " *" } else { "  " };
                    out.push_str(&format!(
                        "\n{marker} {:<14} ({}) {}",
                        info.name, info.source, info.description
                    ));
                }
                out.push_str("\n\nSwitch: ctx_session action=profile value=<name>");
                out
            }
        }

        "budget" => {
            use crate::core::budget_tracker::BudgetTracker;
            let snap = BudgetTracker::global().check();
            let mut out = snap.format_compact();

            if let Ok(data_dir) = crate::core::data_dir::lean_ctx_data_dir() {
                let window = crate::core::context_radar::default_window_for_client("cursor");
                let radar = crate::core::context_radar::ContextRadar::load(&data_dir, window);
                let radar_display = radar.format_display();
                if !radar_display.is_empty() {
                    out.push_str("\n\n");
                    out.push_str(&radar_display);
                }
            }
            out
        }

        "role" => {
            use crate::core::roles;
            if let Some(name) = value {
                match roles::set_active_role(name) {
                    Ok(r) => {
                        crate::core::budget_tracker::BudgetTracker::global().reset();
                        format!(
                            "Role switched to '{name}'.\n\
                             Shell: {}, Budget: {} tokens / {} shell / ${:.2}\n\
                             Tools: {}",
                            r.role.shell_policy,
                            r.limits.max_context_tokens,
                            r.limits.max_shell_invocations,
                            r.limits.max_cost_usd,
                            if r.tools.allowed.iter().any(|a| a == "*") {
                                let denied = if r.tools.denied.is_empty() {
                                    "none".to_string()
                                } else {
                                    format!("denied: {}", r.tools.denied.join(", "))
                                };
                                format!("* (all), {denied}")
                            } else {
                                r.tools.allowed.join(", ")
                            }
                        )
                    }
                    Err(e) => {
                        let available: Vec<String> =
                            roles::list_roles().iter().map(|r| r.name.clone()).collect();
                        format!("{e}. Available: {}", available.join(", "))
                    }
                }
            } else {
                let name = roles::active_role_name();
                let r = roles::active_role();
                let list = roles::list_roles();
                let mut out = format!(
                    "Active role: {name}\n\
                     Description: {}\n\
                     Shell policy: {}, Budget: {} tokens / {} shell / ${:.2}\n\n\
                     Available roles:",
                    r.role.description,
                    r.role.shell_policy,
                    r.limits.max_context_tokens,
                    r.limits.max_shell_invocations,
                    r.limits.max_cost_usd,
                );
                for info in &list {
                    let marker = if info.is_active { " *" } else { "  " };
                    out.push_str(&format!(
                        "\n{marker} {:<14} ({}) {}",
                        info.name, info.source, info.description
                    ));
                }
                out.push_str("\n\nSwitch: ctx_session action=role value=<name>");
                out
            }
        }

        "diff" => {
            let parts: Vec<&str> = value.unwrap_or("").split_whitespace().collect();
            if parts.len() < 2 {
                return "Usage: ctx_session diff <session_id_a> <session_id_b> [format]\n\
                        Formats: summary (default), json\n\
                        Example: ctx_session diff abc123 def456 json"
                    .to_string();
            }
            let id_a = parts[0];
            let id_b = parts[1];
            let format = parts.get(2).copied().unwrap_or("summary");

            let sess_a = SessionState::load_by_id(id_a);
            let sess_b = SessionState::load_by_id(id_b);

            match (sess_a, sess_b) {
                (Some(a), Some(b)) => {
                    let d = crate::core::session_diff::diff_sessions(&a, &b);
                    match format {
                        "json" => d.format_json(),
                        _ => d.format_summary(),
                    }
                }
                (None, _) => format!("Session not found: {id_a}"),
                (_, None) => format!("Session not found: {id_b}"),
            }
        }

        "slo" => {
            match value {
                Some("reload") => {
                    crate::core::slo::reload();
                    "SLO definitions reloaded from disk.".to_string()
                }
                Some("history") => {
                    let hist = crate::core::slo::violation_history(20);
                    if hist.is_empty() {
                        "No SLO violations recorded.".to_string()
                    } else {
                        let mut out = format!("SLO violations (last {}):\n", hist.len());
                        for v in &hist {
                            out.push_str(&format!(
                                "  {} {} ({}) {:.2} vs {:.2} → {}\n",
                                v.timestamp, v.slo_name, v.metric, v.actual, v.threshold, v.action
                            ));
                        }
                        out
                    }
                }
                Some("clear") => {
                    crate::core::slo::clear_violations();
                    "SLO violation history cleared.".to_string()
                }
                _ => {
                    let snap = crate::core::slo::evaluate_quiet();
                    snap.format_compact()
                }
            }
        }

        "output_stats" => {
            let snap = crate::core::output_verification::stats_snapshot();
            snap.format_compact()
        }

        "verify" => {
            let snap = crate::core::output_verification::stats_snapshot();
            format!(
                "DEPRECATION: action=\"verify\" is renamed to action=\"output_stats\" (ctx_verify is the full observability stack).\n{}",
                snap.format_compact()
            )
        }

        "episodes" => {
            let project_root = session.project_root.clone().unwrap_or_else(|| {
                std::env::current_dir().map_or_else(
                    |_| "unknown".to_string(),
                    |p| p.to_string_lossy().to_string(),
                )
            });
            let policy = match crate::core::config::Config::load().memory_policy_effective() {
                Ok(p) => p,
                Err(e) => {
                    let path = crate::core::config::Config::path().map_or_else(
                        || "~/.lean-ctx/config.toml".to_string(),
                        |p| p.display().to_string(),
                    );
                    return format!("Error: invalid memory policy: {e}\nFix: edit {path}");
                }
            };
            let hash = crate::core::project_hash::hash_project_root(&project_root);
            let mut store = crate::core::episodic_memory::EpisodicStore::load_or_create(&hash);

            match value {
                Some("record") => {
                    let ep = crate::core::episodic_memory::create_episode_from_session(
                        session,
                        tool_calls,
                    );
                    let id = ep.id.clone();
                    store.record_episode(ep, &policy.episodic);
                    if let Err(e) = store.save() {
                        return format!("Episode record failed: {e}");
                    }
                    crate::core::events::emit(crate::core::events::EventKind::KnowledgeUpdate {
                        category: "episodic".to_string(),
                        key: id.clone(),
                        action: "record".to_string(),
                    });
                    format!("Episode recorded: {id}")
                }
                Some(v) if v.starts_with("search ") => {
                    let q = v.trim_start_matches("search ").trim();
                    let hits = store.search(q);
                    if hits.is_empty() {
                        return "No episodes matched.".to_string();
                    }
                    let mut out = format!("Episodes matched ({}):", hits.len());
                    for ep in hits.into_iter().take(10) {
                        let task: String = ep.task_description.chars().take(50).collect();
                        out.push_str(&format!(
                            "\n  {} | {} | {} | {}",
                            ep.id,
                            ep.timestamp,
                            ep.outcome.label(),
                            task
                        ));
                    }
                    out
                }
                Some(v) if v.starts_with("file ") => {
                    let f = v.trim_start_matches("file ").trim();
                    let hits = store.by_file(f);
                    let mut out = format!("Episodes for file match '{f}' ({}):", hits.len());
                    for ep in hits.into_iter().take(10) {
                        let task: String = ep.task_description.chars().take(50).collect();
                        out.push_str(&format!(
                            "\n  {} | {} | {} | {}",
                            ep.id,
                            ep.timestamp,
                            ep.outcome.label(),
                            task
                        ));
                    }
                    out
                }
                Some(v) if v.starts_with("outcome ") => {
                    let label = v.trim_start_matches("outcome ").trim();
                    let hits = store.by_outcome(label);
                    let mut out = format!("Episodes outcome '{label}' ({}):", hits.len());
                    for ep in hits.into_iter().take(10) {
                        let task: String = ep.task_description.chars().take(50).collect();
                        out.push_str(&format!("\n  {} | {} | {}", ep.id, ep.timestamp, task));
                    }
                    out
                }
                _ => {
                    let stats = store.stats();
                    let recent = store.recent(10);
                    let mut out = format!(
                        "Episodic memory: {} episodes, success_rate={:.0}%, tokens_total={}\n\nRecent:",
                        stats.total_episodes,
                        stats.success_rate * 100.0,
                        stats.total_tokens
                    );
                    for ep in recent {
                        let task: String = ep.task_description.chars().take(60).collect();
                        out.push_str(&format!(
                            "\n  {} | {} | {} | {}",
                            ep.id,
                            ep.timestamp,
                            ep.outcome.label(),
                            task
                        ));
                    }
                    out.push_str("\n\nActions: ctx_session action=episodes value=record|\"search <q>\"|\"file <path>\"|\"outcome success|failure|partial|unknown\"");
                    out
                }
            }
        }

        "procedures" => {
            let project_root = session.project_root.clone().unwrap_or_else(|| {
                std::env::current_dir().map_or_else(
                    |_| "unknown".to_string(),
                    |p| p.to_string_lossy().to_string(),
                )
            });
            let policy = match crate::core::config::Config::load().memory_policy_effective() {
                Ok(p) => p,
                Err(e) => {
                    let path = crate::core::config::Config::path().map_or_else(
                        || "~/.lean-ctx/config.toml".to_string(),
                        |p| p.display().to_string(),
                    );
                    return format!("Error: invalid memory policy: {e}\nFix: edit {path}");
                }
            };
            let hash = crate::core::project_hash::hash_project_root(&project_root);
            let episodes = crate::core::episodic_memory::EpisodicStore::load_or_create(&hash);
            let mut procs = crate::core::procedural_memory::ProceduralStore::load_or_create(&hash);

            match value {
                Some("detect") => {
                    procs.detect_patterns(&episodes.episodes, &policy.procedural);
                    if let Err(e) = procs.save() {
                        return format!("Procedure detect failed: {e}");
                    }
                    crate::core::events::emit(crate::core::events::EventKind::KnowledgeUpdate {
                        category: "procedural".to_string(),
                        key: hash.clone(),
                        action: "detect".to_string(),
                    });
                    format!(
                        "Procedures updated. Total procedures: {} (episodes: {}).",
                        procs.procedures.len(),
                        episodes.episodes.len()
                    )
                }
                Some(v) if v.starts_with("suggest ") => {
                    let task = v.trim_start_matches("suggest ").trim();
                    let hits = procs.suggest(task);
                    if hits.is_empty() {
                        return "No procedures matched.".to_string();
                    }
                    let mut out = format!("Procedures suggested ({}):", hits.len());
                    for p in hits.into_iter().take(10) {
                        out.push_str(&format!(
                            "\n  {} | conf={:.0}% | success={:.0}% | steps={}",
                            p.name,
                            p.confidence * 100.0,
                            p.success_rate() * 100.0,
                            p.steps.len()
                        ));
                    }
                    out
                }
                _ => {
                    let task = session
                        .task
                        .as_ref()
                        .map(|t| t.description.clone())
                        .unwrap_or_default();
                    let suggestions = if task.is_empty() {
                        Vec::new()
                    } else {
                        procs.suggest(&task)
                    };

                    let mut out = format!(
                        "Procedural memory: {} procedures (episodes: {})",
                        procs.procedures.len(),
                        episodes.episodes.len()
                    );

                    if !task.is_empty() {
                        out.push_str(&format!("\nTask: {}", task.chars().take(80).collect::<String>()));
                        if !suggestions.is_empty() {
                            out.push_str("\n\nSuggested:");
                            for p in suggestions.into_iter().take(5) {
                                out.push_str(&format!(
                                    "\n  {} | conf={:.0}% | success={:.0}% | steps={}",
                                    p.name,
                                    p.confidence * 100.0,
                                    p.success_rate() * 100.0,
                                    p.steps.len()
                                ));
                            }
                        }
                    }

                    out.push_str("\n\nActions: ctx_session action=procedures value=detect|\"suggest <task>\"");
                    out
                }
            }
        }

        _ => format!("Unknown action: {action}. Use: status, load, save, task, finding, decision, reset, list, cleanup, snapshot, restore, resume, configure, profile, role, budget, slo, diff, output_stats, verify, export, import, episodes, procedures"),
    }
}

fn parse_finding_value(value: &str) -> (Option<String>, Option<u32>, &str) {
    // Format: "file.rs:42 — summary text" or just "summary text"
    if let Some(dash_pos) = value.find(" \u{2014} ").or_else(|| value.find(" - ")) {
        let location = &value[..dash_pos];
        let sep_len = 3;
        let text = &value[dash_pos + sep_len..];

        if let Some(colon_pos) = location.rfind(':') {
            let file = &location[..colon_pos];
            if let Ok(line) = location[colon_pos + 1..].parse::<u32>() {
                return (Some(file.to_string()), Some(line), text);
            }
        }
        return (Some(location.to_string()), None, text);
    }
    (None, None, value)
}
