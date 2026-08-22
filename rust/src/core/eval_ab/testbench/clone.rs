//! Repo materialization for the testbench (#611).
//!
//! Turns a [`RepoEntry`] into an on-disk directory the eval can read:
//!
//! * **local fixture** (`path`) — resolved canonically inside the lockfile dir.
//!   This is what the committed deterministic CI subset uses, so it never touches the
//!   network.
//! * **remote** (`url` + `commit`) — cloned into `cache/<name>` once, then checked out
//!   at the pinned commit on every run. The clone is idempotent (reused across runs)
//!   and the checked-out `HEAD` is verified to equal the pin, so a moved tag or a
//!   force-pushed branch can never silently change what the public run measured.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::lockfile::RepoEntry;

/// Materializes `repo` and returns the directory its task workspaces resolve against.
pub fn materialize(repo: &RepoEntry, lock_dir: &Path, cache_dir: &Path) -> Result<PathBuf> {
    if let Err(reason) = repo.validate() {
        bail!("invalid lock entry: {reason}");
    }

    if let Some(rel) = &repo.path {
        let lock_root = std::fs::canonicalize(lock_dir)
            .with_context(|| format!("canonicalizing lock directory {}", lock_dir.display()))?;
        let dir = std::fs::canonicalize(lock_root.join(rel))
            .with_context(|| format!("canonicalizing local fixture {rel}"))?;
        if !dir.starts_with(&lock_root) {
            bail!(
                "repo {}: local fixture {} escapes lock directory",
                repo.name,
                rel
            );
        }
        if !dir.is_dir() {
            bail!(
                "repo {}: local fixture {} is not a directory",
                repo.name,
                dir.display()
            );
        }
        return Ok(dir);
    }

    // Remote: url + commit are guaranteed present by lockfile validation.
    let url = repo
        .url
        .as_deref()
        .context("remote repo entry without url (should be unreachable)")?;
    let commit = repo
        .commit
        .as_deref()
        .context("remote repo entry without commit (should be unreachable)")?;

    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating cache dir {}", cache_dir.display()))?;
    let cache_root = std::fs::canonicalize(cache_dir)
        .with_context(|| format!("canonicalizing cache dir {}", cache_dir.display()))?;
    let dest = cache_root.join(&repo.name);
    validate_cache_destination(&cache_root, &dest)?;

    if !dest.join(".git").is_dir() {
        // Fresh clone. A full clone is heavier than a shallow one but lets us check
        // out an arbitrary pinned commit reliably across git versions; it is paid
        // once and reused on every subsequent run.
        git(&["clone", "--quiet", url, &dest.to_string_lossy()], None)
            .with_context(|| format!("cloning {url} for repo {}", repo.name))?;
        validate_cache_destination(&cache_root, &dest)?;
    }

    // Check out the pin; fetch once if the commit is not present yet (e.g. a newer
    // pin against an existing cache), then retry. A still-missing commit is fatal.
    if git(&["checkout", "--quiet", commit], Some(&dest)).is_err() {
        git(&["fetch", "--quiet", "--all", "--tags"], Some(&dest))
            .with_context(|| format!("fetching {url} for repo {}", repo.name))?;
        git(&["checkout", "--quiet", commit], Some(&dest)).with_context(|| {
            format!("checking out pinned commit {commit} in repo {}", repo.name)
        })?;
    }

    let head = git(&["rev-parse", "HEAD"], Some(&dest))?;
    let head = head.trim();
    if head != commit && !head.starts_with(commit) {
        bail!(
            "repo {}: checked-out HEAD {head} does not match pinned commit {commit}",
            repo.name
        );
    }
    Ok(dest)
}

fn validate_cache_destination(cache_root: &Path, dest: &Path) -> Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(dest) {
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing symlinked testbench cache destination {}",
                dest.display()
            );
        }
        let canonical = std::fs::canonicalize(dest)
            .with_context(|| format!("canonicalizing cache destination {}", dest.display()))?;
        if !canonical.starts_with(cache_root) {
            bail!(
                "testbench cache destination {} escapes cache directory",
                dest.display()
            );
        }
    }

    let git_dir = dest.join(".git");
    if let Ok(metadata) = std::fs::symlink_metadata(&git_dir) {
        if metadata.file_type().is_symlink() {
            bail!("refusing symlinked git directory {}", git_dir.display());
        }
        let canonical = std::fs::canonicalize(&git_dir)
            .with_context(|| format!("canonicalizing git directory {}", git_dir.display()))?;
        if !canonical.starts_with(cache_root) {
            bail!(
                "git directory {} escapes cache directory",
                git_dir.display()
            );
        }
    }
    Ok(())
}

/// Runs `git ARGS` (optionally in `dir`), returning trimmed stdout or an error that
/// includes git's stderr. Never inherits a shell — args are passed verbatim.
fn git(args: &[&str], dir: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.args(args);
    let out = cmd
        .output()
        .with_context(|| format!("spawning git {}", args.join(" ")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_fixture_resolves_against_lock_dir() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("repos/qa")).unwrap();
        let entry = RepoEntry {
            name: "qa".into(),
            url: None,
            commit: None,
            path: Some("repos/qa".into()),
            suite: "qa.ndjson".into(),
        };
        let got = materialize(&entry, root.path(), &root.path().join("cache")).unwrap();
        assert_eq!(
            got,
            std::fs::canonicalize(root.path().join("repos/qa")).unwrap()
        );
    }

    #[test]
    fn missing_local_fixture_errors() {
        let root = tempfile::tempdir().unwrap();
        let entry = RepoEntry {
            name: "qa".into(),
            url: None,
            commit: None,
            path: Some("nope".into()),
            suite: "qa.ndjson".into(),
        };
        assert!(materialize(&entry, root.path(), &root.path().join("cache")).is_err());
    }

    #[test]
    fn rejects_unsafe_paths_before_materialization() {
        let root = tempfile::tempdir().unwrap();
        for rel in ["/tmp/fixture", "../fixture", "nested/../../fixture"] {
            let entry = RepoEntry {
                name: "qa".into(),
                url: None,
                commit: None,
                path: Some(rel.into()),
                suite: "qa.ndjson".into(),
            };
            assert!(materialize(&entry, root.path(), &root.path().join("cache")).is_err());
        }

        let cache = root.path().join("cache");
        let entry = RepoEntry {
            name: "../escape".into(),
            url: Some("https://example.invalid/repo.git".into()),
            commit: Some("deadbeef".into()),
            path: None,
            suite: "qa.ndjson".into(),
        };
        assert!(materialize(&entry, root.path(), &cache).is_err());
        assert!(
            !cache.exists(),
            "unsafe name must fail before cache creation"
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_fixture_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join("fixture");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let entry = RepoEntry {
            name: "qa".into(),
            url: None,
            commit: None,
            path: Some("fixture".into()),
            suite: "qa.ndjson".into(),
        };
        assert!(materialize(&entry, root.path(), &root.path().join("cache")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cache_destination_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, cache.join("repo")).unwrap();
        let entry = RepoEntry {
            name: "repo".into(),
            url: Some("https://example.invalid/repo.git".into()),
            commit: Some("deadbeef".into()),
            path: None,
            suite: "qa.ndjson".into(),
        };
        assert!(materialize(&entry, root.path(), &cache).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn clones_and_checks_out_a_local_git_repo() {
        // A local "remote" git repo is enough to exercise clone + pinned checkout
        // without any network access.
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&origin)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "git {args:?} failed"
            );
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(origin.join("file.txt"), "hello").unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "init"]);
        let commit = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(&origin)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        let entry = RepoEntry {
            name: "fix".into(),
            url: Some(origin.to_string_lossy().into_owned()),
            commit: Some(commit),
            path: None,
            suite: "s.ndjson".into(),
        };
        let cache = root.path().join("cache");
        let dest = materialize(&entry, root.path(), &cache).unwrap();
        assert!(dest.join("file.txt").exists());
        // Second call is idempotent (reuses the clone).
        let dest2 = materialize(&entry, root.path(), &cache).unwrap();
        assert_eq!(dest, dest2);
    }

    #[cfg(unix)]
    #[test]
    fn wrong_pinned_commit_errors() {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            Command::new("git")
                .current_dir(&origin)
                .args(&args)
                .output()
                .unwrap();
        }
        std::fs::write(origin.join("f"), "x").unwrap();
        for args in [vec!["add", "."], vec!["commit", "--quiet", "-m", "i"]] {
            Command::new("git")
                .current_dir(&origin)
                .args(&args)
                .output()
                .unwrap();
        }
        let entry = RepoEntry {
            name: "fix".into(),
            url: Some(origin.to_string_lossy().into_owned()),
            commit: Some("0000000000000000000000000000000000000000".into()),
            path: None,
            suite: "s.ndjson".into(),
        };
        assert!(materialize(&entry, root.path(), &root.path().join("cache")).is_err());
    }
}
