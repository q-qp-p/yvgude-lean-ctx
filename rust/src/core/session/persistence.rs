use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::heuristics::{normalize_loaded_session, session_matches_project_root};
use super::paths::sessions_dir;
use super::state::{BATCH_SAVE_INTERVAL, extract_session_facts};
#[allow(clippy::wildcard_imports)]
use super::types::*;

/// Keep the startup warm set deliberately small: cache warming is an optional
/// optimisation and must never turn process startup into a session-store scan.
const PROJECT_HISTORY_LIMIT: usize = 8;

#[derive(Debug, Default, Deserialize, Serialize)]
struct ProjectSessionIndex {
    version: u8,
    project_root: String,
    /// Oldest to newest; duplicates are removed before appending on every save.
    session_ids: Vec<String>,
}

fn normalized_safe_project_root(project_root: &str) -> Option<String> {
    let path = std::path::Path::new(project_root);
    if project_root.trim().is_empty() || crate::core::pathutil::is_broad_or_unsafe_root(path) {
        return None;
    }
    Some(
        crate::core::pathutil::safe_canonicalize_or_self(path)
            .to_string_lossy()
            .to_string(),
    )
}

fn project_index_path(dir: &std::path::Path, project_root: &str) -> std::path::PathBuf {
    let key = blake3::hash(project_root.as_bytes()).to_hex();
    dir.join("project-index").join(format!("{key}.json"))
}

/// Update one project's bounded warm-history index under a short, local lock.
/// The index is strictly an acceleration structure: a failure never invalidates
/// the already-committed session save.
fn update_project_index(dir: &std::path::Path, project_root: &str, id: &str) -> Result<(), String> {
    use fs2::FileExt;
    use std::io::ErrorKind;
    use std::time::{Duration, Instant};

    const LOCK_TIMEOUT: Duration = Duration::from_millis(200);
    const RETRY_INTERVAL: Duration = Duration::from_millis(10);

    let index_path = project_index_path(dir, project_root);
    let index_dir = index_path.parent().ok_or("project index has no parent")?;
    std::fs::create_dir_all(index_dir).map_err(|e| format!("create project index: {e}"))?;

    let lock_path = index_path.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .map_err(|e| format!("project index lock: {e}"))?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(RETRY_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                return Err("project index lock timed out".to_string());
            }
            Err(error) => return Err(format!("project index lock: {error}")),
        }
    }

    let result = (|| {
        let mut index = std::fs::read_to_string(&index_path)
            .ok()
            .and_then(|json| serde_json::from_str::<ProjectSessionIndex>(&json).ok())
            .filter(|index| index.version == 1 && index.project_root == project_root)
            .unwrap_or_else(|| ProjectSessionIndex {
                version: 1,
                project_root: project_root.to_string(),
                session_ids: Vec::new(),
            });

        index.session_ids.retain(|existing| existing != id);
        index.session_ids.push(id.to_string());
        let excess = index
            .session_ids
            .len()
            .saturating_sub(PROJECT_HISTORY_LIMIT);
        if excess > 0 {
            index.session_ids.drain(..excess);
        }

        let json =
            serde_json::to_string(&index).map_err(|e| format!("serialize project index: {e}"))?;
        let tmp = index_path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, json).map_err(|e| format!("write project index: {e}"))?;
        restrict_file_permissions(&tmp);
        std::fs::rename(tmp, index_path).map_err(|e| format!("commit project index: {e}"))
    })();
    let _ = FileExt::unlock(&lock);
    result
}

#[cfg(unix)]
fn restrict_file_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    let _ = std::fs::set_permissions(path, perms);
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &std::path::Path) {}

fn validate_session_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id == "latest"
        || id.starts_with('.')
        || id.contains('/')
        || id.contains('\\')
        || id.contains(std::path::MAIN_SEPARATOR)
    {
        return Err("invalid session id".to_string());
    }
    Ok(())
}

fn persist_session_facts(session: &SessionState) -> Result<(), String> {
    let Some(project_root) = session
        .project_root
        .as_deref()
        .filter(|project_root| !project_root.trim().is_empty())
    else {
        return Ok(());
    };

    let facts = extract_session_facts(session);
    if facts.is_empty() {
        return Ok(());
    }

    let mut knowledge = crate::core::knowledge::ProjectKnowledge::load_or_create(project_root);
    for fact in facts {
        knowledge.add_fact(fact);
    }
    knowledge.save()
}

impl PreparedSave {
    /// Writes the pre-serialized session data, latest pointer, and compaction
    /// snapshot to disk atomically. A per-session file lock and version check
    /// make deferred saves monotonic even when background tasks finish out of
    /// order.
    pub fn write_to_disk(self) -> Result<(), String> {
        use fs2::FileExt;

        if !self.dir.exists() {
            std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        }
        let lock_path = self.dir.join(format!(".{}.save.lock", self.id));
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .map_err(|e| format!("open session save lock: {e}"))?;
        lock.lock_exclusive()
            .map_err(|e| format!("lock session save: {e}"))?;

        let result = (|| {
            let path = self.dir.join(format!("{}.json", self.id));
            if persisted_session_version(&path).is_some_and(|version| version > self.version) {
                return Ok(());
            }
            let tmp = self.dir.join(format!(".{}.json.tmp", self.id));
            std::fs::write(&tmp, &self.json).map_err(|e| e.to_string())?;
            restrict_file_permissions(&tmp);
            std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;

            let latest_path = self.dir.join("latest.json");
            let latest_tmp = self.dir.join(".latest.json.tmp");
            std::fs::write(&latest_tmp, &self.pointer_json).map_err(|e| e.to_string())?;
            restrict_file_permissions(&latest_tmp);
            std::fs::rename(&latest_tmp, &latest_path).map_err(|e| e.to_string())?;

            if let Some(snapshot) = self.compaction_snapshot {
                let snap_path = self.dir.join(format!("{}_snapshot.txt", self.id));
                if let Err(error) = crate::core::atomic_fs::write_bytes_with_fallback(
                    &snap_path,
                    snapshot.as_bytes(),
                    None,
                ) {
                    tracing::debug!("lean-ctx: compaction snapshot update skipped: {error}");
                } else {
                    restrict_file_permissions(&snap_path);
                }
            }
            if let Some(project_root) = self.project_index_root.as_deref()
                && let Err(error) = update_project_index(&self.dir, project_root, &self.id)
            {
                tracing::debug!("lean-ctx: session warm-history index update skipped: {error}");
            }
            Ok(())
        })();
        let _ = FileExt::unlock(&lock);
        result
    }
}

fn persisted_session_version(path: &std::path::Path) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    value["version"].as_u64()?.try_into().ok()
}

impl SessionState {
    /// Counts locally recorded decisions from the trailing seven days.
    #[must_use]
    pub fn decision_count_this_week() -> u64 {
        let cutoff = Utc::now() - chrono::Duration::days(7);
        Self::list_sessions()
            .into_iter()
            .filter_map(|summary| Self::load_by_id(&summary.id))
            .flat_map(|session| session.decisions)
            .filter(|decision| decision.timestamp >= cutoff)
            .count()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    /// Serializes and writes the session state to disk synchronously.
    pub fn save(&mut self) -> Result<(), String> {
        let prepared = self.prepare_save()?;
        match prepared.write_to_disk() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.stats.unsaved_changes = BATCH_SAVE_INTERVAL;
                Err(e)
            }
        }
    }

    /// Serialize session state while holding the lock (CPU-only), reset the
    /// unsaved counter, and return a `PreparedSave` whose I/O can be deferred
    /// to a background thread via `write_to_disk()`.
    pub fn prepare_save(&mut self) -> Result<PreparedSave, String> {
        if self
            .project_root
            .as_deref()
            .is_some_and(|root| normalized_safe_project_root(root).is_none())
        {
            return Err(
                "refusing to persist a session for a broad or unsafe project root".to_string(),
            );
        }
        let dir = sessions_dir().ok_or("cannot determine home directory")?;
        let compaction_snapshot = if self.stats.total_tool_calls > 0 {
            Some(self.build_compaction_snapshot())
        } else {
            None
        };
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let pointer_json = serde_json::to_string(&LatestPointer {
            id: self.id.clone(),
        })
        .map_err(|e| e.to_string())?;
        self.stats.unsaved_changes = 0;
        // #717: arm the time-based flush window.
        self.last_flush = Some(std::time::Instant::now());
        Ok(PreparedSave {
            dir,
            id: self.id.clone(),
            version: self.version,
            json,
            pointer_json,
            compaction_snapshot,
            project_index_root: self
                .project_root
                .as_deref()
                .and_then(normalized_safe_project_root),
        })
    }

    /// Load the bounded warm-history set for one safe project root.
    ///
    /// There is intentionally no legacy full-store fallback: cache warming is
    /// optional, while scanning every persisted session on each MCP launch is
    /// not acceptable under concurrent agent load. New saves populate the
    /// index; legacy sessions remain available through explicit session tools.
    pub(crate) fn load_recent_for_project_root(project_root: &str, limit: usize) -> Vec<Self> {
        let Some(project_root) = normalized_safe_project_root(project_root) else {
            return Vec::new();
        };
        let Some(dir) = sessions_dir() else {
            return Vec::new();
        };
        let Some(index) = std::fs::read_to_string(project_index_path(&dir, &project_root))
            .ok()
            .and_then(|json| serde_json::from_str::<ProjectSessionIndex>(&json).ok())
            .filter(|index| index.version == 1 && index.project_root == project_root)
        else {
            return Vec::new();
        };

        index
            .session_ids
            .iter()
            .rev()
            .take(limit.min(PROJECT_HISTORY_LIMIT))
            .filter_map(|id| Self::load_by_id(id))
            .collect()
    }

    /// Loads the most recent session matching the current working directory's
    /// project root.
    ///
    /// Returns `None` (a fresh session) rather than falling back to the global
    /// `latest.json` pointer: that unconditional fallback bypassed project-root
    /// matching and was the root cause of cross-project session leakage — one
    /// project's findings/decisions/knowledge bleeding into another project's
    /// first session. The correct project session is loaded later from the MCP
    /// `roots` handshake (`load_latest_for_project_root`).
    ///
    /// Also refuses to scope to a broad/unsafe cwd (e.g. the MCP daemon's HOME),
    /// which would otherwise resurrect the contaminated "HOME mega-session".
    pub fn load_latest() -> Option<Self> {
        let cwd = std::env::current_dir().ok()?;
        if crate::core::pathutil::is_broad_or_unsafe_root(&cwd) {
            return None;
        }
        Self::load_latest_for_project_root(&cwd.to_string_lossy())
    }

    /// Loads the session referenced by the global `latest.json` pointer,
    /// regardless of project. Intended only for explicit, cross-project UX
    /// (e.g. `lean-ctx session` status from an arbitrary directory) — never for
    /// injecting knowledge into a new project's context. Prefer `load_latest`.
    pub fn load_global_latest_pointer() -> Option<Self> {
        let dir = sessions_dir()?;
        let latest_path = dir.join("latest.json");
        let pointer_json = std::fs::read_to_string(&latest_path).ok()?;
        let pointer: LatestPointer = serde_json::from_str(&pointer_json).ok()?;
        Self::load_by_id(&pointer.id)
    }

    /// Loads the most recent session matching a specific project root.
    pub fn load_latest_for_project_root(project_root: &str) -> Option<Self> {
        // Broad roots ("/", HOME, agent sandboxes) never own a session. Bail out
        // BEFORE scanning: the daemon boots with cwd "/" and previously walked
        // every stored session here, stat-ing each session's project_root /
        // shell_cwd. For roots under ~/Documents that probe popped the macOS
        // TCC prompt in lean-ctx's name on every launchd (re)start (#356) —
        // and `shell_cwd.starts_with("/")` could even leak an arbitrary
        // project's session into the broad-root context.
        if crate::core::pathutil::is_broad_or_unsafe_root(std::path::Path::new(project_root)) {
            return None;
        }
        let dir = sessions_dir()?;
        let target_root =
            crate::core::pathutil::safe_canonicalize_or_self(std::path::Path::new(project_root));
        let mut latest_match: Option<Self> = None;

        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("latest.json") {
                continue;
            }

            let Some(id) = path.file_stem().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(session) = Self::load_by_id(id) else {
                continue;
            };

            if !session_matches_project_root(&session, &target_root) {
                continue;
            }

            if latest_match
                .as_ref()
                .is_none_or(|existing| session.updated_at > existing.updated_at)
            {
                latest_match = Some(session);
            }
        }

        latest_match
    }

    /// Loads a specific session from disk by its unique ID.
    pub fn load_by_id(id: &str) -> Option<Self> {
        validate_session_id(id).ok()?;
        let dir = sessions_dir()?;
        let path = dir.join(format!("{id}.json"));
        let json = std::fs::read_to_string(&path).ok()?;
        let session: Self = serde_json::from_str(&json).ok()?;
        Some(normalize_loaded_session(session))
    }

    /// Deletes a saved session and its compaction snapshot.
    ///
    /// If the deleted session is the global latest pointer, the pointer is
    /// moved to the newest remaining session or removed when none remain.
    pub fn delete_session(id: &str) -> Result<bool, String> {
        validate_session_id(id)?;
        let Some(dir) = sessions_dir() else {
            return Ok(false);
        };
        let path = dir.join(format!("{id}.json"));
        if !path.exists() {
            return Ok(false);
        }

        if let Some(session) = Self::load_by_id(id) {
            persist_session_facts(&session)?;
        }
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;

        let snapshot = dir.join(format!("{id}_snapshot.txt"));
        match std::fs::remove_file(&snapshot) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }

        let latest_path = dir.join("latest.json");
        let points_to_deleted = std::fs::read_to_string(&latest_path)
            .ok()
            .and_then(|json| serde_json::from_str::<LatestPointer>(&json).ok())
            .is_some_and(|pointer| pointer.id == id);
        if points_to_deleted {
            if let Some(next) = Self::list_sessions().into_iter().next() {
                let latest_tmp = dir.join(".latest.json.tmp");
                let pointer_json = serde_json::to_string(&LatestPointer { id: next.id })
                    .map_err(|e| e.to_string())?;
                std::fs::write(&latest_tmp, pointer_json).map_err(|e| e.to_string())?;
                restrict_file_permissions(&latest_tmp);
                std::fs::rename(&latest_tmp, &latest_path).map_err(|e| e.to_string())?;
            } else {
                match std::fs::remove_file(&latest_path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }

        Ok(true)
    }

    /// Lists all saved sessions as summaries, sorted by most recently updated.
    pub fn list_sessions() -> Vec<SessionSummary> {
        let Some(dir) = sessions_dir() else {
            return Vec::new();
        };

        let mut summaries = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if path.file_name().and_then(|n| n.to_str()) == Some("latest.json") {
                    continue;
                }
                if let Ok(json) = std::fs::read_to_string(&path)
                    && let Ok(session) = serde_json::from_str::<SessionState>(&json)
                {
                    summaries.push(SessionSummary {
                        id: session.id,
                        started_at: session.started_at,
                        updated_at: session.updated_at,
                        version: session.version,
                        task: session.task.as_ref().map(|t| t.description.clone()),
                        tool_calls: session.stats.total_tool_calls,
                        tokens_saved: session.stats.total_tokens_saved,
                        project_root: session.project_root,
                    });
                }
            }
        }

        summaries.sort_by_key(|x| std::cmp::Reverse(x.updated_at));
        summaries
    }

    /// Scans all saved sessions for contaminated ones — those rooted at a
    /// broad/unsafe path (HOME, filesystem root, agent sandbox dir) without a
    /// real project marker, i.e. the historic "HOME mega-session" artifact.
    ///
    /// Returns `(found, quarantined)` where `found` is `(id, root)` pairs. When
    /// `apply` is true, each offending session file is moved to a
    /// `sessions/quarantine/` subdirectory (non-destructive) instead of being
    /// loaded into any project's context.
    pub fn doctor_quarantine_unsafe_roots(apply: bool) -> (Vec<(String, String)>, usize) {
        let mut found: Vec<(String, String)> = Vec::new();
        let mut quarantined = 0usize;
        let Some(dir) = sessions_dir() else {
            return (found, quarantined);
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return (found, quarantined);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|n| n.to_str()) else {
                continue;
            };
            if id == "latest" || id.starts_with('.') {
                continue;
            }
            let Some(session) = Self::load_by_id(id) else {
                continue;
            };
            let Some(root) = session.project_root.as_deref() else {
                continue;
            };
            let root_path = std::path::Path::new(root);
            if crate::core::pathutil::is_broad_or_unsafe_root(root_path) {
                found.push((id.to_string(), root.to_string()));
                if apply {
                    let q_dir = dir.join("quarantine");
                    if std::fs::create_dir_all(&q_dir).is_ok()
                        && std::fs::rename(&path, q_dir.join(format!("{id}.json"))).is_ok()
                    {
                        quarantined += 1;
                    }
                }
            }
        }
        (found, quarantined)
    }

    /// Deletes sessions older than `max_age_days`, preserving the latest. Returns count removed.
    pub fn cleanup_old_sessions(max_age_days: i64) -> u32 {
        let Some(dir) = sessions_dir() else { return 0 };

        let cutoff = Utc::now() - chrono::Duration::days(max_age_days);
        let latest = Self::load_latest().map(|s| s.id);
        let mut removed = 0u32;

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let filename = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
                if filename == "latest" || filename.starts_with('.') {
                    continue;
                }
                if latest.as_deref() == Some(filename) {
                    continue;
                }
                if let Ok(json) = std::fs::read_to_string(&path)
                    && let Ok(session) = serde_json::from_str::<SessionState>(&json)
                    && session.updated_at < cutoff
                    && std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|content| serde_json::from_str::<Self>(&content).ok())
                        .is_none_or(|session| persist_session_facts(&session).is_ok())
                    && std::fs::remove_file(&path).is_ok()
                {
                    removed += 1;
                }
            }
        }

        removed
    }
}

#[cfg(test)]
mod tests {
    use super::SessionState;

    #[test]
    fn recent_project_sessions_use_bounded_index_without_scanning_legacy_store() {
        let _data = crate::core::data_dir::isolated_data_dir();
        let project = tempfile::tempdir().expect("project tempdir");
        let root = project.path().to_string_lossy().to_string();

        for id in ["first", "second", "third"] {
            let mut session = SessionState::new();
            session.id = id.to_string();
            session.project_root = Some(root.clone());
            session.save().expect("save indexed session");
        }

        // A malformed legacy artifact proves the hot path consults only the
        // per-project index, never `list_sessions()` as a hidden fallback.
        let sessions = crate::core::session::paths::sessions_dir().expect("sessions dir");
        std::fs::write(sessions.join("legacy-unreadable.json"), "not json")
            .expect("write legacy artifact");

        let ids: Vec<_> = SessionState::load_recent_for_project_root(&root, 8)
            .into_iter()
            .map(|session| session.id)
            .collect();
        assert_eq!(ids, ["third", "second", "first"]);
    }

    #[test]
    fn recent_project_sessions_refuse_broad_roots() {
        let _data = crate::core::data_dir::isolated_data_dir();
        assert!(SessionState::load_recent_for_project_root("/", 8).is_empty());
    }

    #[test]
    fn broad_root_sessions_are_never_persisted() {
        let _data = crate::core::data_dir::isolated_data_dir();
        let mut session = SessionState::new();
        session.project_root = Some("/".to_string());

        let error = session
            .save()
            .expect_err("broad root save must be rejected");

        assert!(error.contains("broad or unsafe"));
        assert!(
            crate::core::session::paths::sessions_dir()
                .expect("sessions dir")
                .read_dir()
                .map_or(true, |mut entries| entries.next().is_none())
        );
    }

    #[test]
    fn deferred_save_cannot_replace_a_newer_session_version() {
        let _data = crate::core::data_dir::isolated_data_dir();
        let mut session = SessionState::new();
        let id = session.id.clone();
        let older = session.prepare_save().expect("prepare older save");
        session.increment();
        let expected_version = session.version;
        let newer = session.prepare_save().expect("prepare newer save");

        newer.write_to_disk().expect("write newer save");
        older.write_to_disk().expect("skip older save");

        assert_eq!(
            SessionState::load_by_id(&id)
                .expect("load persisted session")
                .version,
            expected_version
        );
    }
}
