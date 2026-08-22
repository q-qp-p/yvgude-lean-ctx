//! Background agent reaper: periodically GCs dead presence processes, expired
//! scratchpad entries, and stale logical sessions.
//!
//! Spawned once by the daemon; runs until the process exits.
//! Reaper TTLs are configured through `[agents]`; interval wiring remains pending.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static RUNNING: OnceLock<AtomicBool> = OnceLock::new();

/// Default reaper interval (10 minutes).
const DEFAULT_INTERVAL: Duration = Duration::from_mins(10);
fn load_config() -> crate::core::config::AgentsConfig {
    crate::core::config::Config::load().agents
}

/// Spawn the background reaper thread. Safe to call multiple times -- only the
/// first call starts the thread; subsequent calls are no-ops.
pub(crate) fn spawn_reaper() {
    let flag = RUNNING.get_or_init(|| AtomicBool::new(false));
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("agent-reaper".to_string())
        .spawn(move || reaper_loop(DEFAULT_INTERVAL))
        .ok();
}

fn reaper_loop(interval: Duration) {
    loop {
        std::thread::sleep(interval);
        if let Err(error) = reap_cycle() {
            tracing::warn!("agent reaper cycle failed: {error}");
        }
    }
}

/// Run one reap cycle. Public for testing.
pub(crate) fn reap_cycle() -> Result<ReapStats, String> {
    let mut stats = ReapStats::default();
    let cfg = load_config();

    // Presence registry: cleanup_stale marks dead PIDs as Finished and removes
    // old Finished entries.
    if let Ok((_registry, ())) = super::AgentRegistry::mutate_locked(|registry| {
        let agents_before = registry.agents.len();
        let scratchpad_before = registry.scratchpad.len();

        // cleanup_stale handles: dead PIDs → Finished, old agents removal,
        // AND expired scratchpad entries (since #502).
        registry.cleanup_stale(cfg.presence_ttl_hours);

        stats.presence_removed = agents_before.saturating_sub(registry.agents.len());
        stats.scratchpad_expired = scratchpad_before.saturating_sub(registry.scratchpad.len());

        // Logical sessions: cleanup stale.
        let sessions_before = registry.logical_sessions.len();
        registry.cleanup_stale_logical_sessions(cfg.logical_session_ttl_seconds);
        stats.sessions_expired = sessions_before.saturating_sub(registry.logical_sessions.len());
    }) {}

    tracing::debug!(
        "reaper: presence={} scratchpad={} sessions={}",
        stats.presence_removed,
        stats.scratchpad_expired,
        stats.sessions_expired,
    );

    Ok(stats)
}

/// Statistics from one reap cycle.
#[derive(Debug, Default, Clone)]
pub(crate) struct ReapStats {
    pub presence_removed: usize,
    pub scratchpad_expired: usize,
    pub sessions_expired: usize,
}

impl ReapStats {
    pub(crate) fn total(&self) -> usize {
        self.presence_removed + self.scratchpad_expired + self.sessions_expired
    }
}

#[cfg(test)]
mod tests {
    use super::{reap_cycle, spawn_reaper};

    #[test]
    fn spawn_reaper_is_idempotent() {
        spawn_reaper();
        spawn_reaper();
    }

    #[test]
    fn reap_cycle_succeeds_on_empty_registries() {
        let _isolated_data_dir = crate::core::data_dir::isolated_data_dir();
        let stats = reap_cycle().expect("reap on empty");
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn reap_cycle_removes_expired_scratchpad() {
        use super::super::{AgentRegistry, ScratchpadEntry};
        use crate::core::a2a::message::{MessagePriority, PrivacyLevel};

        let _isolated_data_dir = crate::core::data_dir::isolated_data_dir();
        AgentRegistry::mutate_locked(|registry| {
            registry.scratchpad.push(ScratchpadEntry {
                id: "expired-1".to_string(),
                from_agent: "a".to_string(),
                to_agent: None,
                task_id: None,
                category: "test".to_string(),
                priority: MessagePriority::default(),
                privacy: PrivacyLevel::default(),
                message: "old".to_string(),
                metadata: std::collections::HashMap::new(),
                project_root: None,
                timestamp: chrono::Utc::now(),
                read_by: vec![],
                expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
            });
        })
        .expect("setup");

        let stats = reap_cycle().expect("reap");
        assert!(stats.scratchpad_expired >= 1);
    }
}
