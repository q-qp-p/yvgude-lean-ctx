use super::AgentRegistry;
use std::path::PathBuf;

pub(super) fn agents_dir() -> Result<PathBuf, String> {
    let dir = crate::core::data_dir::lean_ctx_data_dir()?;
    Ok(dir.join("agents"))
}

pub(super) fn mutate_persistent<T>(
    mutate: impl FnOnce(&mut AgentRegistry) -> T,
) -> Result<T, String> {
    let dir = agents_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _lock = FileLock::acquire(&dir.join("registry.lock"))?;
    let path = dir.join("registry.json");
    let mut registry = load_registry_file(&path)?.unwrap_or_default();
    let result = mutate(&mut registry);
    save_registry_file(&path, &registry)?;
    Ok(result)
}

/// A missing registry is the normal first-run case. A malformed registry is a
/// data-integrity failure, not an empty bus: callers that mutate must fail
/// closed instead of overwriting messages and presence with a blank snapshot.
pub(super) fn load_registry_file(path: &std::path::Path) -> Result<Option<AgentRegistry>, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .map(Some)
            .map_err(|error| format!("agent registry is corrupt at {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read agent registry {}: {error}", path.display())),
    }
}

pub(super) fn save_registry_file(
    path: &std::path::Path,
    registry: &AgentRegistry,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(registry).map_err(|error| error.to_string())?;
    crate::config_io::write_atomic(path, &json)
        .map_err(|error| format!("persist agent registry {}: {error}", path.display()))
}

pub(super) fn generate_short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// #576 already fixed this exact hardcoded-`true` anti-pattern for
/// `daemon::is_daemon_running` by delegating to `ipc::process::is_alive`
/// (which has a real Windows `OpenProcess` check); this duplicate copy was
/// missed, so on non-unix targets `cleanup_stale` could never flip a dead
/// MCP session's entry to `Finished`, leaving `registry.json` accumulating
/// stale `Active` entries forever — the root cause of the "N active agents"
/// dashboard bug on Windows.
pub(crate) fn is_process_alive(pid: u32) -> bool {
    crate::ipc::process::is_alive(pid)
}

pub(crate) struct FileLock {
    file: std::fs::File,
}

impl FileLock {
    pub(crate) fn acquire(path: &std::path::Path) -> Result<Self, String> {
        use fs2::FileExt;

        const TIMEOUT: std::time::Duration = std::time::Duration::from_millis(750);
        const RETRY: std::time::Duration = std::time::Duration::from_millis(25);

        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| format!("agent registry lock {}: {error}", path.display()))?;
        let deadline = std::time::Instant::now() + TIMEOUT;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(RETRY);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Err(format!(
                        "agent registry lock timed out after {}ms",
                        TIMEOUT.as_millis()
                    ));
                }
                Err(error) => return Err(format!("agent registry lock: {error}")),
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
