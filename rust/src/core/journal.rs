use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use chrono::{Local, Utc};

/// The activity journal is diagnostic state, never an unbounded hot-path
/// workload. Archives preserve history while the active file stays cheap to
/// inspect and update.
const MAX_ACTIVE_JOURNAL_BYTES: u64 = 4 * 1024 * 1024;
const JOURNAL_TAIL_BYTES: u64 = 8 * 1024;
const JOURNAL_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

static JOURNAL_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn journal_path() -> PathBuf {
    crate::core::paths::state_dir()
        .unwrap_or_else(|_| PathBuf::from(".lean-ctx"))
        .join("journal.md")
}

fn journal_day_path() -> PathBuf {
    crate::core::paths::state_dir()
        .unwrap_or_else(|_| PathBuf::from(".lean-ctx"))
        .join("journal.day")
}

fn journal_lock_path() -> PathBuf {
    crate::core::paths::state_dir()
        .unwrap_or_else(|_| PathBuf::from(".lean-ctx"))
        .join("journal.lock")
}

/// Serialize journal maintenance briefly across MCP processes. Journaling is
/// observational, so a contended lock is skipped instead of delaying a tool.
fn with_journal_lock(f: impl FnOnce()) {
    use fs2::FileExt;
    use std::io::ErrorKind;

    let _local = JOURNAL_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(lock) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(journal_lock_path())
    else {
        return;
    };
    let deadline = std::time::Instant::now() + JOURNAL_LOCK_WAIT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error)
                if error.kind() == ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_) => return,
        }
    }
    f();
    let _ = FileExt::unlock(&lock);
}

fn active_journal_needs_rotation(path: &std::path::Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.len() >= MAX_ACTIVE_JOURNAL_BYTES)
        .unwrap_or(false)
}

fn rotate_active_journal_if_needed(path: &std::path::Path) {
    if !active_journal_needs_rotation(path) {
        return;
    }
    let archive_name = format!("journal-{}.md", Utc::now().format("%Y%m%dT%H%M%S%6fZ"));
    let archive = path.with_file_name(archive_name);
    if std::fs::rename(path, archive).is_ok() {
        let _ = std::fs::remove_file(journal_day_path());
    }
}

fn write_day_marker(today: &str) {
    let _ = std::fs::write(journal_day_path(), today);
}

fn journal_tail_contains(path: &std::path::Path, needle: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let start = file
        .metadata()
        .map(|metadata| metadata.len().saturating_sub(JOURNAL_TAIL_BYTES))
        .unwrap_or(0);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).is_ok() && String::from_utf8_lossy(&tail).contains(needle)
}

fn is_enabled() -> bool {
    if let Ok(v) = std::env::var("LEAN_CTX_JOURNAL") {
        return !matches!(v.trim(), "0" | "false" | "off");
    }
    super::config::Config::load().journal_enabled
}

/// Append a human-readable entry to the activity journal.
pub fn log(category: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    with_journal_lock(|| {
        let path = journal_path();
        rotate_active_journal_if_needed(&path);
        let timestamp = Local::now().format("%Y-%m-%d %H:%M");
        let entry = format!("- **{timestamp}** [{category}] {message}\n");
        let needs_header = !path.exists();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path);

        if let Ok(mut f) = file {
            if needs_header {
                let date = Local::now().format("%Y-%m-%d").to_string();
                let _ = writeln!(f, "# lean-ctx Activity Journal\n\n## {date}\n");
                write_day_marker(&date);
            }
            let _ = f.write_all(entry.as_bytes());
        }
    });
}

/// Insert a day separator if the last entry was on a different date.
pub fn maybe_day_separator() {
    if !is_enabled() {
        return;
    }
    with_journal_lock(|| {
        let path = journal_path();
        if !path.exists() {
            return;
        }

        let today = Local::now().format("%Y-%m-%d").to_string();
        if std::fs::read_to_string(journal_day_path()).is_ok_and(|stored| stored.trim() == today) {
            return;
        }

        let header = format!("## {today}");
        if !journal_tail_contains(&path, &header)
            && let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path)
        {
            let _ = writeln!(f, "\n{header}\n");
        }
        write_day_marker(&today);
    });
}

/// Log a tool call to the journal.
pub fn log_tool_call(tool_name: &str, summary: &str) {
    if matches!(
        tool_name,
        "ctx_session" | "ctx_knowledge" | "ctx_context" | "ctx_radar"
    ) {
        return;
    }
    log("tool", &format!("`{tool_name}` — {summary}"));
}

/// Return the journal content for display.
pub fn read_journal(tail_lines: usize) -> String {
    let path = journal_path();
    if !path.exists() {
        return "No journal entries yet.".to_string();
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    if tail_lines == 0 {
        return content;
    }
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    lines[start..].join("\n")
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn journal_log_creates_file() {
        // `journal.md` is STATE (GH #408); isolated_data_dir collapses all four
        // category dirs onto one temp dir so the write/read pair stays valid.
        let iso = crate::core::data_dir::isolated_data_dir();
        crate::test_env::set_var("LEAN_CTX_JOURNAL", "1");

        log("test", "hello world");

        let path = iso.path().join("journal.md");
        assert!(path.exists(), "journal.md should be created");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[test] hello world"));
        assert!(content.contains("# lean-ctx Activity Journal"));

        crate::test_env::remove_var("LEAN_CTX_JOURNAL");
    }

    #[test]
    fn read_journal_tail() {
        let _iso = crate::core::data_dir::isolated_data_dir();
        crate::test_env::set_var("LEAN_CTX_JOURNAL", "1");

        for i in 0..5 {
            log("test", &format!("entry {i}"));
        }

        let tail = read_journal(2);
        assert!(tail.contains("entry 4"), "should contain last entry");
        assert!(
            !tail.contains("Activity Journal"),
            "should not contain header"
        );

        crate::test_env::remove_var("LEAN_CTX_JOURNAL");
    }

    #[test]
    fn oversized_active_journal_rotates_before_append() {
        let iso = crate::core::data_dir::isolated_data_dir();
        crate::test_env::set_var("LEAN_CTX_JOURNAL", "1");
        let journal = iso.path().join("journal.md");
        std::fs::write(
            &journal,
            vec![b'x'; (MAX_ACTIVE_JOURNAL_BYTES + 1) as usize],
        )
        .expect("write oversized journal");

        log("test", "after rotation");

        assert!(
            std::fs::read_to_string(&journal)
                .expect("new active journal")
                .contains("after rotation")
        );
        assert!(
            std::fs::read_dir(iso.path())
                .expect("journal directory")
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("journal-"))
        );
        crate::test_env::remove_var("LEAN_CTX_JOURNAL");
    }

    #[test]
    fn day_separator_uses_small_tail_and_persists_marker() {
        let iso = crate::core::data_dir::isolated_data_dir();
        crate::test_env::set_var("LEAN_CTX_JOURNAL", "1");
        let today = Local::now().format("%Y-%m-%d").to_string();
        let journal = iso.path().join("journal.md");
        std::fs::write(&journal, format!("{}\n## {today}\n", "x".repeat(16 * 1024)))
            .expect("write journal");

        maybe_day_separator();

        assert_eq!(
            std::fs::read_to_string(iso.path().join("journal.day"))
                .expect("day marker")
                .trim(),
            today
        );
        crate::test_env::remove_var("LEAN_CTX_JOURNAL");
    }
}
