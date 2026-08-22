//! Unified identity and MCP-process presence view for the OCLA wire API.
//!
//! Durable identities intentionally do not claim process liveness. The
//! presence registry remains the sole source for PID-backed `alive` state.

use std::collections::HashSet;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UnifiedAgent {
    pub agent_id: String,
    pub source: AgentSource,
    pub role: Option<String>,
    pub status: String,
    pub pid: Option<u32>,
    pub alive: bool,
    pub last_active: Option<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentSource {
    Identity,
    Presence,
    Both,
}

/// Merge durable identities with live MCP-process presence.
pub(crate) fn list_unified() -> Vec<UnifiedAgent> {
    let mut result = Vec::new();
    let mut seen_ids = HashSet::new();

    for record in crate::core::agent_registry::list() {
        let status = match record.status {
            crate::core::agent_registry::AgentStatus::Active => "active",
            crate::core::agent_registry::AgentStatus::Suspended => "suspended",
            crate::core::agent_registry::AgentStatus::Decommissioned => "decommissioned",
        };
        seen_ids.insert(record.agent_id.clone());
        result.push(UnifiedAgent {
            agent_id: record.agent_id,
            source: AgentSource::Identity,
            role: Some(record.role),
            status: status.to_string(),
            pid: None,
            alive: false,
            last_active: record.last_heartbeat,
            owner: Some(record.owner),
        });
    }

    if let Some(registry) = super::AgentRegistry::load() {
        for agent in &registry.agents {
            if seen_ids.contains(&agent.agent_id) {
                if let Some(existing) = result
                    .iter_mut()
                    .find(|item| item.agent_id == agent.agent_id)
                {
                    existing.source = AgentSource::Both;
                    existing.pid = Some(agent.pid);
                    existing.alive = crate::ipc::process::is_alive(agent.pid);
                }
                continue;
            }

            let alive = crate::ipc::process::is_alive(agent.pid);
            result.push(UnifiedAgent {
                agent_id: agent.agent_id.clone(),
                source: AgentSource::Presence,
                role: agent.role.clone(),
                status: if alive {
                    agent.status.to_string()
                } else {
                    "stale".to_string()
                },
                pid: Some(agent.pid),
                alive,
                last_active: Some(agent.last_active.to_rfc3339()),
                owner: None,
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::list_unified;

    #[test]
    fn list_unified_returns_empty_on_fresh_install() {
        let _iso = crate::core::data_dir::isolated_data_dir();
        let result = list_unified();
        assert!(result.is_empty());
    }
}
