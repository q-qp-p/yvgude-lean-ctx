//! First-class agent identities: registry + lifecycle (GL #433, H3 Epic D).
//!
//! An agent stops being an anonymous process with a role config and
//! becomes a registered identity: stable `agent_id`, mandatory human
//! `owner` (accountability principle — orphaned agents are the security
//! hole of the agent era), lifecycle state, best-effort attestation and a
//! SPIFFE-compatible identity string for workload-IAM integration.
//!
//! Storage: `<data_dir>/agents/identity-registry.json`, advisory-file-locked like
//! the audit trail (multiple concurrent agent processes are LeanCTX's
//! normal operating mode). Every lifecycle transition writes a
//! tamper-evident audit entry (OCP Part 4, additive event types).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::audit_trail::{self, AuditEntryData, AuditEventType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentStatus {
    Active,
    Suspended,
    Decommissioned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Attestation {
    /// SHA-256 of the running binary at registration/heartbeat time.
    pub binary_sha256: String,
    /// SHA-256 of the active role file (empty when the role is built-in).
    pub config_sha256: String,
    pub attested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentRecord {
    /// Stable identity (key of the registry).
    pub agent_id: String,
    /// Role name under `roles/*.toml` / built-ins.
    pub role: String,
    /// Human accountable for this agent — mandatory, never empty.
    pub owner: String,
    pub status: AgentStatus,
    pub created_at: String,
    /// Ed25519 public key (hex) bound to this identity.
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attestation: Option<Attestation>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_heartbeat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub suspended_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub decommissioned_at: Option<String>,
}

/// Outcome of an identity check on a call path (team server middleware,
/// enforce mode).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct IdentityCheck {
    pub agent_id: String,
    pub registered: bool,
    /// Active = may act. Suspended/decommissioned/unregistered = may not.
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AgentStatus>,
    pub detail: String,
}

fn registry_path() -> Result<PathBuf, String> {
    let dir = crate::core::data_dir::lean_ctx_data_dir()
        .map_err(|e| format!("data dir: {e}"))?
        .join("agents");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("identity-registry.json"))
}

fn legacy_registry_path() -> Result<PathBuf, String> {
    registry_path().map(|path| path.with_file_name("registry.json"))
}

fn load_unlocked(path: &PathBuf) -> BTreeMap<String, AgentRecord> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn load_registry(path: &PathBuf) -> BTreeMap<String, AgentRecord> {
    if path.exists() {
        return load_unlocked(path);
    }
    legacy_registry_path()
        .map(|legacy| load_unlocked(&legacy))
        .unwrap_or_default()
}

/// Run `f` over the registry under an exclusive cross-process lock and
/// persist the result.
fn with_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, AgentRecord>) -> Result<T, String>,
) -> Result<T, String> {
    use fs2::FileExt;
    let path = registry_path()?;
    let lock_path = path.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("registry lock: {e}"))?;
    lock.lock_exclusive()
        .map_err(|e| format!("registry lock: {e}"))?;

    let mut registry = load_registry(&path);
    let result = f(&mut registry);
    if result.is_ok() {
        let json =
            serde_json::to_string_pretty(&registry).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&path, json).map_err(|e| format!("persist registry: {e}"))?;
    }
    let _ = FileExt::unlock(&lock);
    result
}

/// Read-only registry snapshot.
pub(crate) fn list() -> Vec<AgentRecord> {
    registry_path()
        .map(|p| load_registry(&p).into_values().collect())
        .unwrap_or_default()
}

/// Active identities are the peer view. Lifecycle history remains available
/// through `list()` for audit and operator use.
pub(crate) fn list_active() -> Vec<AgentRecord> {
    list()
        .into_iter()
        .filter(|record| record.status == AgentStatus::Active)
        .collect()
}

pub(crate) fn get(agent_id: &str) -> Option<AgentRecord> {
    registry_path()
        .ok()
        .and_then(|p| load_registry(&p).remove(agent_id))
}

fn audit(event_type: AuditEventType, agent_id: &str, role: &str, detail: Option<String>) {
    audit_trail::record(AuditEntryData {
        agent_id: agent_id.to_string(),
        tool: "agent_registry".to_string(),
        action: detail,
        input_hash: audit_trail::hash_input(&serde_json::Map::new()),
        output_tokens: 0,
        role: role.to_string(),
        event_type,
    });
}

/// SHA-256 of the running binary, cached by (len, mtime) — heartbeats may
/// fire every minute and the binary is large; re-hashing is only needed
/// when the file on disk actually changed (which is exactly the drift
/// signal we care about, and it changes the mtime).
fn binary_sha256() -> String {
    use std::sync::{Mutex, OnceLock};
    /// (binary len, binary mtime secs) → hex digest.
    type HashCache = Mutex<Option<((u64, u64), String)>>;
    static CACHE: OnceLock<HashCache> = OnceLock::new();

    let Ok(exe) = std::env::current_exe() else {
        return String::new();
    };
    let Ok(meta) = std::fs::metadata(&exe) else {
        return String::new();
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let key = (meta.len(), mtime);

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut slot = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((cached_key, hash)) = slot.as_ref()
        && *cached_key == key
    {
        return hash.clone();
    }
    let hash = std::fs::read(&exe)
        .map(|bytes| sha256_hex(&bytes))
        .unwrap_or_default();
    *slot = Some((key, hash.clone()));
    hash
}

/// Best-effort attestation: hash the running binary and the role file.
/// Detects drift; does NOT stop a determined attacker who controls the
/// host (documented in docs/enterprise/agent-identity.md).
pub(crate) fn attest(role: &str) -> Attestation {
    let binary_sha256 = binary_sha256();
    let config_sha256 = role_file_path(role)
        .and_then(|p| std::fs::read(p).ok())
        .map(|bytes| sha256_hex(&bytes))
        .unwrap_or_default();
    Attestation {
        binary_sha256,
        config_sha256,
        attested_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn role_file_path(role: &str) -> Option<PathBuf> {
    let dir = crate::core::data_dir::lean_ctx_data_dir()
        .ok()?
        .join("roles");
    let path = dir.join(format!("{role}.toml"));
    path.exists().then_some(path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    crate::core::agent_identity::hex_encode(&hasher.finalize())
}

/// Register a new agent identity. The role must exist; the owner is
/// mandatory (accountability). Creating an identity also provisions its
/// Ed25519 keypair.
pub(crate) fn register(agent_id: &str, role: &str, owner: &str) -> Result<AgentRecord, String> {
    if agent_id.trim().is_empty()
        || !agent_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("agent_id must be non-empty [A-Za-z0-9_-]".to_string());
    }
    if owner.trim().is_empty() {
        return Err(
            "owner is mandatory — every agent identity has a human accountable for it".to_string(),
        );
    }
    if crate::core::roles::load_role(role).is_none() {
        return Err(format!(
            "role '{role}' does not exist (see `lean-ctx roles list`)"
        ));
    }

    let public_key = crate::core::agent_identity::get_public_key(agent_id)
        .map(|k| crate::core::agent_identity::hex_encode(k.as_bytes()))
        .map_err(|e| format!("keypair: {e}"))?;

    let record = AgentRecord {
        agent_id: agent_id.to_string(),
        role: role.to_string(),
        owner: owner.trim().to_string(),
        status: AgentStatus::Active,
        created_at: chrono::Utc::now().to_rfc3339(),
        public_key,
        attestation: Some(attest(role)),
        last_heartbeat: None,
        suspended_reason: None,
        decommissioned_at: None,
    };

    with_registry(|reg| {
        if reg.contains_key(agent_id) {
            return Err(format!("agent '{agent_id}' is already registered"));
        }
        reg.insert(agent_id.to_string(), record.clone());
        Ok(())
    })?;
    audit(
        AuditEventType::AgentRegistered,
        agent_id,
        role,
        Some(format!("owner={}", record.owner)),
    );
    Ok(record)
}

/// Heartbeat: liveness + re-attestation. Returns drift against the
/// registration-time attestation, if any.
pub(crate) fn heartbeat(agent_id: &str) -> Result<Option<String>, String> {
    with_registry(|reg| {
        let record = reg
            .get_mut(agent_id)
            .ok_or_else(|| format!("agent '{agent_id}' is not registered"))?;
        if record.status == AgentStatus::Decommissioned {
            return Err(format!("agent '{agent_id}' is decommissioned"));
        }
        let fresh = attest(&record.role);
        let drift = match &record.attestation {
            Some(prev) if prev.binary_sha256 != fresh.binary_sha256 => {
                Some("binary hash changed since registration".to_string())
            }
            Some(prev) if prev.config_sha256 != fresh.config_sha256 => {
                Some("role config changed since registration".to_string())
            }
            _ => None,
        };
        record.last_heartbeat = Some(fresh.attested_at.clone());
        Ok(drift)
    })
}

pub(crate) fn suspend(agent_id: &str, reason: &str) -> Result<(), String> {
    let role = transition(agent_id, AgentStatus::Suspended, Some(reason.to_string()))?;
    audit(
        AuditEventType::AgentSuspended,
        agent_id,
        &role,
        Some(reason.to_string()),
    );
    Ok(())
}

pub(crate) fn resume(agent_id: &str) -> Result<(), String> {
    let role = transition(agent_id, AgentStatus::Active, None)?;
    audit(AuditEventType::AgentResumed, agent_id, &role, None);
    Ok(())
}

/// Decommission closes the identity with a final audit entry; the record
/// stays in the registry (auditability) but can never act again.
pub(crate) fn decommission(agent_id: &str) -> Result<(), String> {
    let role = with_registry(|reg| {
        let record = reg
            .get_mut(agent_id)
            .ok_or_else(|| format!("agent '{agent_id}' is not registered"))?;
        record.status = AgentStatus::Decommissioned;
        record.decommissioned_at = Some(chrono::Utc::now().to_rfc3339());
        Ok(record.role.clone())
    })?;
    audit(
        AuditEventType::AgentDecommissioned,
        agent_id,
        &role,
        Some("audit-closing entry".to_string()),
    );
    Ok(())
}

fn transition(agent_id: &str, to: AgentStatus, reason: Option<String>) -> Result<String, String> {
    with_registry(|reg| {
        let record = reg
            .get_mut(agent_id)
            .ok_or_else(|| format!("agent '{agent_id}' is not registered"))?;
        if record.status == AgentStatus::Decommissioned {
            return Err(format!(
                "agent '{agent_id}' is decommissioned — identities are never reactivated"
            ));
        }
        record.status = to;
        record.suspended_reason = reason;
        Ok(record.role.clone())
    })
}

/// Owner offboarding (SCIM `active=false` hook, GL #399): suspend every
/// active agent owned by `owner`. Returns the suspended agent ids.
pub(crate) fn suspend_agents_for_owner(owner: &str, reason: &str) -> Result<Vec<String>, String> {
    let suspended = with_registry(|reg| {
        let mut hit = Vec::new();
        for record in reg.values_mut() {
            if record.owner == owner && record.status == AgentStatus::Active {
                record.status = AgentStatus::Suspended;
                record.suspended_reason = Some(reason.to_string());
                hit.push((record.agent_id.clone(), record.role.clone()));
            }
        }
        Ok(hit)
    })?;
    for (agent_id, role) in &suspended {
        audit(
            AuditEventType::AgentSuspended,
            agent_id,
            role,
            Some(format!("owner offboarded: {reason}")),
        );
    }
    Ok(suspended.into_iter().map(|(id, _)| id).collect())
}

/// Identity check for enforce paths (team-server middleware): registered
/// AND active. Unregistered agents are reported (monitor mode logs,
/// enforce mode rejects — the caller decides).
pub(crate) fn check(agent_id: &str) -> IdentityCheck {
    match get(agent_id) {
        None => IdentityCheck {
            agent_id: agent_id.to_string(),
            registered: false,
            allowed: false,
            status: None,
            detail: "not registered — register with `lean-ctx agent register`".to_string(),
        },
        Some(record) => {
            let allowed = record.status == AgentStatus::Active;
            IdentityCheck {
                agent_id: agent_id.to_string(),
                registered: true,
                allowed,
                status: Some(record.status),
                detail: match record.status {
                    AgentStatus::Active => format!("active, owner {}", record.owner),
                    AgentStatus::Suspended => format!(
                        "suspended: {}",
                        record.suspended_reason.as_deref().unwrap_or("no reason")
                    ),
                    AgentStatus::Decommissioned => "decommissioned".to_string(),
                },
            }
        }
    }
}

/// SPIFFE-compatible workload identity:
/// `spiffe://<trust_domain>/agent/<role>/<agent_id>`.
pub(crate) fn spiffe_id(record: &AgentRecord, trust_domain: &str) -> String {
    format!(
        "spiffe://{}/agent/{}/{}",
        trust_domain.trim_matches('/'),
        record.role,
        record.agent_id
    )
}

#[cfg(test)]
#[path = "agent_registry_tests.rs"]
mod tests;
