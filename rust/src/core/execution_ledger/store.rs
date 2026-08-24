//! File-backed append-only execution ledger.
//! The append journal proves local crash continuity; signed canonical receipts,
//! not this unkeyed sidecar, provide adversarial authenticity.

use std::fs;
#[cfg(any(unix, test))]
use std::fs::File;
#[cfg(test)]
use std::fs::OpenOptions;
#[cfg(any(unix, test))]
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(unix)]
use fs2::FileExt;
#[cfg(any(unix, test))]
use serde::de::Error as _;
#[cfg(any(unix, test))]
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
#[cfg(any(unix, test))]
use serde::{Deserialize, Serialize};
#[cfg(any(unix, test))]
use serde_json::{Number, Value};
#[cfg(any(unix, test))]
use sha2::{Digest, Sha256};

use super::event::ExecutionEvent;
#[cfg(any(unix, test))]
use super::verify::{GENESIS, hash_event, verify_events};
use super::{ExecutionLedgerError, Result};

#[cfg(any(unix, test))]
const LEDGER_RECORD_SCHEMA: &str = "lean-ctx.execution-ledger-record.v1";
#[cfg(any(unix, test))]
const LEDGER_RECORD_KIND: &str = "execution_event";
#[cfg(any(unix, test))]
const APPEND_JOURNAL_SCHEMA: &str = "lean-ctx.execution-ledger-append-journal.v1";
#[cfg(any(unix, test))]
const MAX_LEDGER_RECORD_BYTES: usize = 1024 * 1024;
#[cfg(any(unix, test))]
const MAX_APPEND_JOURNAL_BYTES: usize = MAX_LEDGER_RECORD_BYTES * 2 + 1024;

#[cfg(any(unix, test))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LedgerRecordV1 {
    schema: String,
    kind: String,
    event: ExecutionEvent,
    entry_hash: String,
}

#[cfg(any(unix, test))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AppendJournalV1 {
    schema: String,
    previous_len: u64,
    previous_sha256: String,
    record: String,
    record_sha256: String,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    dev: u64,
    ino: u64,
}

#[cfg(not(unix))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity;

/// Default execution-ledger location: `<data_dir>/execution/ledger.jsonl`.
pub fn default_path() -> Option<PathBuf> {
    let data_dir = crate::core::data_dir::lean_ctx_data_dir().ok()?;
    Some(data_dir.join("execution").join("ledger.jsonl"))
}

/// A process-independent handle to an execution ledger file.
#[derive(Debug)]
pub struct ExecutionLedgerStore {
    path: PathBuf,
    trusted_root: PathBuf,
    relative_path: PathBuf,
    trusted_root_identity: Option<DirectoryIdentity>,
    trusted_parent_identity: Arc<Mutex<Option<DirectoryIdentity>>>,
}

impl Clone for ExecutionLedgerStore {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            trusted_root: self.trusted_root.clone(),
            relative_path: self.relative_path.clone(),
            trusted_root_identity: self.trusted_root_identity,
            trusted_parent_identity: Arc::clone(&self.trusted_parent_identity),
        }
    }
}

impl PartialEq for ExecutionLedgerStore {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.trusted_root == other.trusted_root
            && self.relative_path == other.relative_path
            && self.trusted_root_identity == other.trusted_root_identity
    }
}

impl Eq for ExecutionLedgerStore {}

impl ExecutionLedgerStore {
    /// Creates a store backed by `path`.
    ///
    /// This compatibility constructor treats the lexical parent of `path` as
    /// the trusted root.  Every operation still opens each component from a
    /// descriptor with no-follow semantics; callers that receive an external
    /// path should prefer [`Self::new_verified`], which makes the root and
    /// relative path explicit and rejects traversal components up front.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let requested = path.into();
        let path = if requested.is_absolute() {
            requested
        } else {
            std::env::current_dir()
                .map(|directory| directory.join(&requested))
                .unwrap_or(requested)
        };
        let trusted_root = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let relative_path = path
            .file_name()
            .map_or_else(|| PathBuf::from(""), PathBuf::from);
        let trusted_root_identity = capture_directory_identity(&trusted_root).ok();
        Self {
            path,
            trusted_root,
            relative_path,
            trusted_root_identity,
            trusted_parent_identity: Arc::new(Mutex::new(trusted_root_identity)),
        }
    }

    /// Creates a store rooted at `trusted_root` and addressed by `relative`.
    ///
    /// The root is never traversed through a symlink and `relative` may not be
    /// absolute or contain `..`; opened descriptors are checked again after
    /// acquisition so a concurrent rename cannot redirect a read.
    pub fn new_verified(
        trusted_root: impl Into<PathBuf>,
        relative: impl Into<PathBuf>,
    ) -> Result<Self> {
        let trusted_root = trusted_root.into();
        let relative_path = relative.into();
        validate_root_path(&trusted_root)?;
        validate_relative_path(&relative_path)?;
        if !trusted_root.is_absolute() {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger trusted root must be absolute".to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let trusted_root = normalize_absolute_root_path(&trusted_root)?;
            let trusted_root_directory = unix_open_root(&trusted_root)?;
            let trusted_root_identity = directory_identity(&trusted_root_directory)?;
            let trusted_parent_identity =
                capture_relative_parent_identity(&trusted_root, &relative_path)?;
            Ok(Self {
                path: trusted_root.join(&relative_path),
                trusted_root,
                relative_path,
                trusted_root_identity: Some(trusted_root_identity),
                trusted_parent_identity: Arc::new(Mutex::new(trusted_parent_identity)),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (trusted_root, relative_path);
            Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger descriptor-relative access is unsupported on this platform"
                    .to_owned(),
            ))
        }
    }

    /// Creates a store at the configured default location.
    pub fn from_default() -> Result<Self> {
        let data_dir = crate::core::data_dir::lean_ctx_data_dir().map_err(|_| {
            ExecutionLedgerError::InvalidRecord("data directory unavailable".to_owned())
        })?;
        ensure_default_data_root(&data_dir)?;
        Self::new_verified(data_dir, Path::new("execution/ledger.jsonl"))
    }

    #[cfg(unix)]
    fn open_operation(&self, create_parent: bool) -> Result<Operation> {
        let expected_parent = *self.trusted_parent_identity.lock().map_err(|_| {
            ExecutionLedgerError::InvalidRecord(
                "execution ledger identity lock poisoned".to_owned(),
            )
        })?;
        unix_open_operation(
            &self.trusted_root,
            &self.relative_path,
            self.trusted_root_identity,
            expected_parent,
            create_parent,
        )
        .and_then(|operation| {
            let identity = directory_identity(&operation.parent)?;
            let mut expected = self.trusted_parent_identity.lock().map_err(|_| {
                ExecutionLedgerError::InvalidRecord(
                    "execution ledger identity lock poisoned".to_owned(),
                )
            })?;
            if let Some(previous) = *expected {
                if previous != identity {
                    return Err(ExecutionLedgerError::InvalidRecord(
                        "execution ledger parent changed while opening".to_owned(),
                    ));
                }
            } else {
                *expected = Some(identity);
            }
            Ok(operation)
        })
    }

    #[cfg(unix)]
    fn open_operation_for_read(&self) -> Result<Option<Operation>> {
        if self.trusted_root_identity.is_some() {
            // Validate the constructor-captured identity before attempting any
            // descriptor acquisition; a replaced root is never treated as empty.
            validate_directory_identity(&self.trusted_root, &unix_open_root(&self.trusted_root)?)?;
        }
        let parent_pinned = *self.trusted_parent_identity.lock().map_err(|_| {
            ExecutionLedgerError::InvalidRecord(
                "execution ledger identity lock poisoned".to_owned(),
            )
        })?;
        match self.open_operation(false) {
            Ok(operation) => Ok(Some(operation)),
            Err(ExecutionLedgerError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound && parent_pinned.is_none() =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Returns the backing path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one event, assigning its next sequence number and chain link.
    pub fn append(&self, event: ExecutionEvent) -> Result<()> {
        self.append_if_new(event).map(drop)
    }

    /// Appends one event and reports whether durable state changed.
    pub(crate) fn append_if_new(&self, event: ExecutionEvent) -> Result<bool> {
        #[cfg(not(unix))]
        {
            let _ = event;
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger descriptor-relative access is unsupported on this platform"
                    .to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let mut event = event;
            let operation = self.open_operation(true)?;
            let mut file = open_ledger_for_append_operation(&operation)?;
            file.lock_exclusive()?;

            let result = (|| {
                validate_operation_file(
                    &operation,
                    &self.relative_path,
                    &file,
                    "execution ledger",
                    true,
                )?;
                recover_pending_append_operation(&mut file, &operation)?;
                let events = read_events_from_file(&file)?;
                if !verify_events(&events)? {
                    return Err(ExecutionLedgerError::InvalidChain(
                        "cannot append to an invalid chain".to_owned(),
                    ));
                }
                if let Some(identity) = event.idempotency_key()
                    && let Some(existing) = events
                        .iter()
                        .find(|existing| existing.idempotency_key() == Some(identity))
                {
                    return if existing.payload_json()? == event.payload_json()? {
                        Ok(false)
                    } else {
                        Err(ExecutionLedgerError::InvalidRecord(
                            "event identity already exists with different payload".to_owned(),
                        ))
                    };
                }

                let previous_hash = match events.last() {
                    Some(previous) => hash_event(previous)?,
                    None => GENESIS.to_owned(),
                };
                let sequence_number = events
                    .last()
                    .map_or(1, ExecutionEvent::sequence_number)
                    .checked_add(u64::from(!events.is_empty()))
                    .ok_or_else(|| {
                        ExecutionLedgerError::InvalidRecord(
                            "execution ledger sequence number overflow".to_owned(),
                        )
                    })?;
                event.set_chain_fields(sequence_number, previous_hash);
                let entry_hash = hash_event(&event)?;
                event.set_entry_hash(entry_hash.clone());
                let mut candidate = events;
                candidate.push(event);
                if !verify_events(&candidate)? {
                    return Err(ExecutionLedgerError::InvalidChain(
                        "event violates execution lifecycle".to_owned(),
                    ));
                }
                let event = candidate.pop().expect("candidate contains appended event");
                let line = serde_json::to_string(&LedgerRecordV1 {
                    schema: LEDGER_RECORD_SCHEMA.to_owned(),
                    kind: LEDGER_RECORD_KIND.to_owned(),
                    event,
                    entry_hash,
                })?;
                let (previous_len, previous_sha256) = file_len_and_sha256(&file)?;
                #[cfg(unix)]
                write_append_journal_operation(&operation, previous_len, &previous_sha256, &line)?;
                file.seek(SeekFrom::End(0))?;
                writeln!(file, "{line}")?;
                file.flush()?;
                file.sync_data()?;
                #[cfg(unix)]
                operation.sync_parent()?;
                #[cfg(unix)]
                validate_operation_file(
                    &operation,
                    &self.relative_path,
                    &file,
                    "execution ledger",
                    true,
                )?;
                #[cfg(unix)]
                clear_append_journal_operation(&operation)?;
                #[cfg(unix)]
                validate_operation_file(
                    &operation,
                    &self.relative_path,
                    &file,
                    "execution ledger",
                    true,
                )?;
                Ok(true)
            })();

            let _ = FileExt::unlock(&file);
            result
        }
    }

    /// Verifies the complete on-disk chain under a shared file lock.
    pub fn verify_chain(&self) -> Result<bool> {
        #[cfg(not(unix))]
        {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger descriptor-relative access is unsupported on this platform"
                    .to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let Some(operation) = self.open_operation_for_read()? else {
                return Ok(true);
            };
            ensure_no_pending_append_operation(&operation)?;
            let Some(file) = open_regular_nofollow_operation(&operation, &self.relative_path)?
            else {
                return Ok(true);
            };
            file.lock_shared()?;
            let result = match ensure_no_pending_append_operation(&operation)
                .and_then(|()| read_events_from_file(&file))
            {
                Ok(events) => validate_operation_file(
                    &operation,
                    &self.relative_path,
                    &file,
                    "execution ledger",
                    true,
                )
                .and_then(|()| verify_events(&events)),
                Err(ExecutionLedgerError::InvalidChain(_)) => Ok(false),
                Err(error) => Err(error),
            };
            let _ = FileExt::unlock(&file);
            result
        }
    }

    /// Loads all well-formed events in file order.
    pub fn load(&self) -> Result<Vec<ExecutionEvent>> {
        #[cfg(not(unix))]
        {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger descriptor-relative access is unsupported on this platform"
                    .to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let Some(operation) = self.open_operation_for_read()? else {
                return Ok(Vec::new());
            };
            ensure_no_pending_append_operation(&operation)?;
            let Some(file) = open_regular_nofollow_operation(&operation, &self.relative_path)?
            else {
                return Ok(Vec::new());
            };
            file.lock_shared()?;
            let result = ensure_no_pending_append_operation(&operation)
                .and_then(|()| read_events_from_file(&file))
                .and_then(|events| {
                    validate_operation_file(
                        &operation,
                        &self.relative_path,
                        &file,
                        "execution ledger",
                        true,
                    )?;
                    Ok(events)
                });
            let _ = FileExt::unlock(&file);
            result
        }
    }

    /// Loads one snapshot only when its record digests, chain, and lifecycle verify.
    pub fn load_verified(&self) -> Result<Vec<ExecutionEvent>> {
        #[cfg(not(unix))]
        {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger descriptor-relative access is unsupported on this platform"
                    .to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let Some(operation) = self.open_operation_for_read()? else {
                return Ok(Vec::new());
            };
            ensure_no_pending_append_operation(&operation)?;
            let Some(file) = open_regular_nofollow_operation(&operation, &self.relative_path)?
            else {
                return Ok(Vec::new());
            };
            file.lock_shared()?;
            let result = ensure_no_pending_append_operation(&operation)
                .and_then(|()| read_events_from_file(&file))
                .and_then(|events| {
                    validate_operation_file(
                        &operation,
                        &self.relative_path,
                        &file,
                        "execution ledger",
                        true,
                    )?;
                    if verify_events(&events)? {
                        Ok(events)
                    } else {
                        Err(ExecutionLedgerError::InvalidChain(
                            "execution ledger chain or lifecycle is invalid".to_owned(),
                        ))
                    }
                });
            let _ = FileExt::unlock(&file);
            result
        }
    }

    /// Returns verified events associated with `task_id`, propagating unavailable state.
    pub fn by_task_verified(&self, task_id: &str) -> Result<Vec<ExecutionEvent>> {
        Ok(self
            .load_verified()?
            .into_iter()
            .filter(|event| event.task_id() == task_id)
            .collect())
    }

    /// Compatibility projection; exact consumers must use [`Self::by_task_verified`].
    #[must_use]
    pub fn by_task(&self, task_id: &str) -> Vec<ExecutionEvent> {
        self.by_task_verified(task_id).unwrap_or_default()
    }

    /// Returns the verified last sequence number, or zero for an empty/missing file.
    pub fn last_sequence_verified(&self) -> Result<u64> {
        Ok(self
            .load_verified()?
            .last()
            .map_or(0, ExecutionEvent::sequence_number))
    }

    /// Compatibility projection; exact consumers must use [`Self::last_sequence_verified`].
    #[must_use]
    pub fn last_sequence(&self) -> u64 {
        self.last_sequence_verified().unwrap_or(0)
    }
}

fn validate_root_path(root: &Path) -> Result<()> {
    if root.as_os_str().is_empty()
        || root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted root is invalid".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn normalize_absolute_root_path(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted root must be absolute".to_owned(),
        ));
    }
    let mut components = root.components();
    let mut normalized = PathBuf::from(std::path::MAIN_SEPARATOR.to_string());
    let first = components.next();
    let first_name = first.and_then(|component| match component {
        std::path::Component::RootDir => components.next(),
        _ => None,
    });
    if let Some(std::path::Component::Normal(name)) = first_name
        && (name == "var" || name == "tmp")
    {
        let alias = Path::new("/").join(name);
        if let Ok(target) = fs::read_link(alias) {
            if target.is_absolute() {
                normalized = target;
            } else {
                normalized.push(target);
            }
        } else {
            normalized.push(name);
        }
    } else if let Some(std::path::Component::Normal(name)) = first_name {
        normalized.push(name);
    }
    for component in components {
        match component {
            std::path::Component::Normal(name) => normalized.push(name),
            std::path::Component::CurDir | std::path::Component::RootDir => {}
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(ExecutionLedgerError::InvalidRecord(
                    "execution ledger trusted root is invalid".to_owned(),
                ));
            }
        }
    }
    Ok(normalized)
}

fn validate_relative_path(relative: &Path) -> Result<()> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
        || !relative
            .components()
            .any(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger relative path is invalid".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn relative_sibling(relative: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = relative.file_name().ok_or_else(|| {
        ExecutionLedgerError::InvalidRecord("execution ledger path has no file name".to_owned())
    })?;
    let mut name = file_name.to_os_string();
    name.push(suffix);
    let mut sibling = relative.to_path_buf();
    sibling.set_file_name(name);
    validate_relative_path(&sibling)?;
    Ok(sibling)
}

#[cfg(unix)]
struct Operation {
    root: File,
    parent: File,
    root_path: PathBuf,
    parent_path: PathBuf,
    parent_relative: PathBuf,
    relative: PathBuf,
}

#[cfg(unix)]
impl Operation {
    fn leaf_name(&self, relative: &Path) -> Result<std::ffi::CString> {
        if relative.parent().unwrap_or_else(|| Path::new("")) != self.parent_relative {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger operation escaped its parent".to_owned(),
            ));
        }
        let name = relative.file_name().ok_or_else(|| {
            ExecutionLedgerError::InvalidRecord("execution ledger path has no file name".to_owned())
        })?;
        use std::os::unix::ffi::OsStrExt;
        std::ffi::CString::new(name.as_bytes()).map_err(|_| {
            ExecutionLedgerError::InvalidRecord("execution ledger path contains NUL".to_owned())
        })
    }

    fn validate_paths(&self) -> Result<()> {
        validate_directory_identity(&self.root_path, &self.root)?;
        validate_directory_identity(&self.parent_path, &self.parent)?;
        Ok(())
    }

    fn sync_parent(&self) -> Result<()> {
        self.parent.sync_all()?;
        self.validate_paths()
    }
}

#[cfg(unix)]
fn directory_identity(file: &File) -> Result<DirectoryIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_dir() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted root is not a directory".to_owned(),
        ));
    }
    Ok(DirectoryIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn capture_directory_identity(path: &Path) -> Result<DirectoryIdentity> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted root is not a directory".to_owned(),
        ));
    }
    Ok(DirectoryIdentity)
}

#[cfg(unix)]
fn capture_directory_identity(path: &Path) -> Result<DirectoryIdentity> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted root is not a directory".to_owned(),
        ));
    }
    use std::os::unix::fs::MetadataExt;
    Ok(DirectoryIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(unix)]
fn validate_directory_identity(path: &Path, directory: &File) -> Result<()> {
    let expected = directory_identity(directory)?;
    let actual = capture_directory_identity(path)?;
    if actual != expected {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted directory changed while opening".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn capture_relative_parent_identity(
    root: &Path,
    relative: &Path,
) -> Result<Option<DirectoryIdentity>> {
    let root_directory = unix_open_root(root)?;
    match unix_open_parent_from_root(&root_directory, relative, false) {
        Ok(parent) => Ok(Some(directory_identity(&parent)?)),
        Err(ExecutionLedgerError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn ensure_default_data_root(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger data root must be absolute".to_owned(),
        ));
    }
    let (absolute, components) = unix_trusted_root_components(path)?;
    let mut directory = unix_open_start(absolute)?;
    for component in components {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let name = CString::new(component.as_bytes()).map_err(|_| {
            ExecutionLedgerError::InvalidRecord("execution ledger path contains NUL".to_owned())
        })?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        // SAFETY: directory is live and name is NUL-terminated.
        let mut fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error.into());
            }
            // SAFETY: directory is live and name is NUL-terminated.
            let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if result < 0 {
                let mkdir_error = std::io::Error::last_os_error();
                if mkdir_error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(mkdir_error.into());
                }
            } else {
                directory.sync_all()?;
            }
            // SAFETY: directory is live and name is NUL-terminated.
            fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        }
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: fd is a new descriptor owned by the returned File.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    directory.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn ensure_default_data_root(path: &Path) -> Result<()> {
    let _ = path;
    Err(ExecutionLedgerError::InvalidRecord(
        "execution ledger descriptor-relative access is unsupported on this platform".to_owned(),
    ))
}

#[cfg(test)]
fn path_parts(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let root = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let relative = path.file_name().map(PathBuf::from).ok_or_else(|| {
        ExecutionLedgerError::InvalidRecord("execution ledger path has no file name".to_owned())
    })?;
    validate_root_path(&root)?;
    validate_relative_path(&relative)?;
    Ok((root, relative))
}

#[cfg(unix)]
fn unix_open_start(absolute: bool) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;

    let start = if absolute {
        b"/".as_slice()
    } else {
        b".".as_slice()
    };
    let start = CString::new(start).expect("static path has no NUL");
    // SAFETY: start is a NUL-terminated static path.
    let fd = unsafe {
        libc::open(
            start.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fd is a new descriptor owned by the returned File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn unix_open_root(root: &Path) -> Result<File> {
    let (absolute, components) = unix_trusted_root_components(root)?;
    let mut directory = unix_open_start(absolute)?;
    for component in components {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let name = CString::new(component.as_bytes()).map_err(|_| {
            ExecutionLedgerError::InvalidRecord("execution ledger path contains NUL".to_owned())
        })?;
        // SAFETY: directory is live and name is NUL-terminated.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: fd is a new descriptor owned by the returned File.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(unix)]
fn unix_open_parent_from_root(
    root: &File,
    relative: &Path,
    create_directories: bool,
) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let components = unix_normal_components(relative)?;
    let directory_components = components
        .get(..components.len().saturating_sub(1))
        .unwrap_or_default();
    let mut directory = root.try_clone()?;
    for component in directory_components {
        let name = CString::new(component.as_bytes()).map_err(|_| {
            ExecutionLedgerError::InvalidRecord("execution ledger path contains NUL".to_owned())
        })?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        // SAFETY: directory is live and name is NUL-terminated.
        let mut fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 && create_directories {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                // SAFETY: directory is live and name is NUL-terminated.
                let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                if result < 0 {
                    let mkdir_error = std::io::Error::last_os_error();
                    if mkdir_error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(mkdir_error.into());
                    }
                } else {
                    directory.sync_all()?;
                }
                // SAFETY: directory is live and name is NUL-terminated.
                fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
            }
        }
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: fd is a new descriptor owned by the returned File.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(unix)]
fn unix_open_operation(
    root_path: &Path,
    relative: &Path,
    expected_root: Option<DirectoryIdentity>,
    expected_parent: Option<DirectoryIdentity>,
    create_parent: bool,
) -> Result<Operation> {
    validate_root_path(root_path)?;
    validate_relative_path(relative)?;
    let root = unix_open_root(root_path)?;
    let root_identity = directory_identity(&root)?;
    if expected_root.is_some_and(|expected| expected != root_identity) {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger trusted root changed while opening".to_owned(),
        ));
    }
    let parent = unix_open_parent_from_root(&root, relative, create_parent)?;
    let parent_identity = directory_identity(&parent)?;
    if expected_parent.is_some_and(|expected| expected != parent_identity) {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger parent changed while opening".to_owned(),
        ));
    }
    let parent_relative = relative
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    let operation = Operation {
        root,
        parent,
        root_path: root_path.to_path_buf(),
        parent_path: root_path.join(&parent_relative),
        parent_relative,
        relative: relative.to_path_buf(),
    };
    operation.validate_paths()?;
    Ok(operation)
}

#[cfg(unix)]
fn open_regular_nofollow_operation(operation: &Operation, relative: &Path) -> Result<Option<File>> {
    let Some(file) = open_nofollow_operation(operation, relative)? else {
        return Ok(None);
    };
    validate_operation_file(operation, relative, &file, "execution ledger", true)?;
    Ok(Some(file))
}

#[cfg(unix)]
fn open_nofollow_operation(operation: &Operation, relative: &Path) -> Result<Option<File>> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = operation.leaf_name(relative)?;
    // SAFETY: parent is live and name is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            operation.parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error.into());
    }
    // SAFETY: fd is a new descriptor owned by the returned File.
    let file = unsafe { File::from_raw_fd(fd) };
    Ok(Some(file))
}

#[cfg(unix)]
fn open_ledger_for_append_operation(operation: &Operation) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = operation.leaf_name(&operation.relative)?;
    loop {
        // SAFETY: parent is live and name is NUL-terminated.
        let fd = unsafe {
            libc::openat(
                operation.parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_APPEND
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK,
            )
        };
        if fd >= 0 {
            // SAFETY: fd is a new descriptor owned by the returned File.
            let file = unsafe { File::from_raw_fd(fd) };
            validate_operation_file(
                operation,
                &operation.relative,
                &file,
                "execution ledger",
                true,
            )?;
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error.into());
        }
        ensure_no_pending_append_operation(operation)?;
        // SAFETY: parent is live and name is NUL-terminated.
        let fd = unsafe {
            libc::openat(
                operation.parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_APPEND
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CREAT
                    | libc::O_EXCL,
                0o600,
            )
        };
        if fd >= 0 {
            // SAFETY: fd is a new descriptor owned by the returned File.
            let file = unsafe { File::from_raw_fd(fd) };
            validate_operation_file(
                operation,
                &operation.relative,
                &file,
                "execution ledger",
                true,
            )?;
            operation.sync_parent()?;
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            continue;
        }
        return Err(error.into());
    }
}

#[cfg(unix)]
fn operation_stat(operation: &Operation, relative: &Path) -> Result<libc::stat> {
    use std::os::fd::AsRawFd;
    let name = operation.leaf_name(relative)?;
    // SAFETY: zeroed stat is initialized by fstatat before it is read.
    let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
    // SAFETY: parent is live, name is NUL-terminated, metadata is writable.
    let result = unsafe {
        libc::fstatat(
            operation.parent.as_raw_fd(),
            name.as_ptr(),
            &raw mut metadata,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(metadata)
}

#[cfg(unix)]
fn validate_operation_file(
    operation: &Operation,
    relative: &Path,
    file: &File,
    label: &str,
    require_single_link: bool,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    operation.validate_paths()?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(ExecutionLedgerError::InvalidRecord(format!(
            "{label} path is not a regular file"
        )));
    }
    if require_single_link && metadata.nlink() != 1 {
        return Err(ExecutionLedgerError::InvalidRecord(format!(
            "{label} has multiple hard links"
        )));
    }
    let path_metadata = operation_stat(operation, relative)?;
    let mode = path_metadata.st_mode as libc::mode_t;
    if mode & libc::S_IFMT != libc::S_IFREG
        || path_metadata.st_dev as u64 != metadata.dev()
        || path_metadata.st_ino as u64 != metadata.ino()
        || path_metadata.st_nlink as u64 != metadata.nlink()
    {
        return Err(ExecutionLedgerError::InvalidRecord(format!(
            "{label} changed while opening"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn create_regular_operation(operation: &Operation, relative: &Path, label: &str) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    operation.validate_paths()?;
    let name = operation.leaf_name(relative)?;
    // SAFETY: parent is live and name is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            operation.parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fd is a new descriptor owned by the returned File.
    let file = unsafe { File::from_raw_fd(fd) };
    validate_operation_file(operation, relative, &file, label, true)?;
    Ok(file)
}

#[cfg(unix)]
fn unlink_operation(operation: &Operation, relative: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;
    let name = operation.leaf_name(relative)?;
    // SAFETY: parent is live and name is NUL-terminated.
    let result = unsafe { libc::unlinkat(operation.parent.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        operation.validate_paths()
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(unix)]
fn remove_file_operation(operation: &Operation, relative: &Path) -> Result<bool> {
    let Some(file) = open_nofollow_operation(operation, relative)? else {
        return Ok(false);
    };
    validate_operation_file(operation, relative, &file, "cleanup target", true)?;
    match unlink_operation(operation, relative) {
        Ok(()) => Ok(true),
        Err(ExecutionLedgerError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn hard_link_operation(operation: &Operation, source: &Path, destination: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;
    let source_name = operation.leaf_name(source)?;
    let destination_name = operation.leaf_name(destination)?;
    let Some(source_file) = open_nofollow_operation(operation, source)? else {
        return Err(ExecutionLedgerError::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )));
    };
    validate_operation_file(
        operation,
        source,
        &source_file,
        "append journal temporary",
        true,
    )?;
    operation.validate_paths()?;
    // SAFETY: both names and parent descriptor are valid and live.
    let result = unsafe {
        libc::linkat(
            operation.parent.as_raw_fd(),
            source_name.as_ptr(),
            operation.parent.as_raw_fd(),
            destination_name.as_ptr(),
            0,
        )
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    operation.validate_paths()
}

#[cfg(unix)]
fn unix_normal_components(path: &Path) -> Result<Vec<std::ffi::OsString>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => components.push(name.to_owned()),
            std::path::Component::CurDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {}
            std::path::Component::ParentDir => {
                return Err(ExecutionLedgerError::InvalidRecord(
                    "execution ledger path contains parent traversal".to_owned(),
                ));
            }
        }
    }
    Ok(components)
}

#[cfg(unix)]
fn unix_trusted_root_components(root: &Path) -> Result<(bool, Vec<std::ffi::OsString>)> {
    let absolute = root.is_absolute();
    let mut components = unix_normal_components(root)?;
    if absolute
        && let Some(first) = components.first()
        && (first == "var" || first == "tmp")
    {
        // macOS exposes /var and /tmp as stable system aliases. Resolve only
        // this first component; user-controlled parent symlinks remain denied
        // by the descriptor-relative O_NOFOLLOW traversal below.
        let alias = Path::new("/").join(first);
        if let Ok(target) = fs::read_link(alias) {
            let mut replacement = unix_normal_components(&target)?;
            replacement.extend(components.into_iter().skip(1));
            components = replacement;
        }
    }
    Ok((absolute, components))
}

#[cfg(any(unix, test))]
fn read_events_from_file(file: &File) -> Result<Vec<ExecutionEvent>> {
    let mut source = file.try_clone()?;
    source.seek(SeekFrom::Start(0))?;
    let (events, tail) = read_complete_events_and_tail(source)?;
    if !tail.is_empty() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger has an unterminated final record".to_owned(),
        ));
    }
    Ok(events)
}

#[cfg(unix)]
fn ensure_no_pending_append_operation(operation: &Operation) -> Result<()> {
    for suffix in [".append-journal", ".append-journal.tmp"] {
        let pending = relative_sibling(&operation.relative, suffix)?;
        if open_nofollow_operation(operation, &pending)?.is_some() {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger has a pending append journal".to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn read_complete_events_and_tail(source: impl Read) -> Result<(Vec<ExecutionEvent>, Vec<u8>)> {
    let mut reader = BufReader::new(source);
    let mut events = Vec::new();
    let mut line_number = 0;
    loop {
        let Some((mut line, terminated)) = read_bounded_line(&mut reader)? else {
            return Ok((events, Vec::new()));
        };
        if !terminated {
            return Ok((events, line));
        }
        line.pop();
        let line = std::str::from_utf8(&line).map_err(|_| {
            ExecutionLedgerError::InvalidRecord(format!(
                "ledger record at index {line_number} is not UTF-8"
            ))
        })?;
        if line.trim().is_empty() {
            return Err(ExecutionLedgerError::InvalidRecord(format!(
                "empty line at index {line_number}"
            )));
        }
        let event = parse_canonical_event(line)?;
        events.push(event);
        line_number += 1;
    }
}

#[cfg(any(unix, test))]
fn read_bounded_line(reader: &mut impl BufRead) -> Result<Option<(Vec<u8>, bool)>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some((line, false)))
            };
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(buffer.len(), |index| index + 1);
        if line.len().saturating_add(consumed) > MAX_LEDGER_RECORD_BYTES + 1 {
            return Err(ExecutionLedgerError::InvalidRecord(
                "execution ledger record exceeds byte limit".to_owned(),
            ));
        }
        line.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some((line, true)));
        }
    }
}

#[cfg(unix)]
fn recover_pending_append_operation(file: &mut File, operation: &Operation) -> Result<()> {
    let Some(journal) = read_append_journal_operation(operation)? else {
        if !file_is_newline_terminated(file)? {
            return Err(ExecutionLedgerError::InvalidRecord(
                "unterminated ledger tail has no matching append journal".to_owned(),
            ));
        }
        let temporary = relative_sibling(&operation.relative, ".append-journal.tmp")?;
        if remove_file_operation(operation, &temporary)? {
            operation.sync_parent()?;
        }
        return Ok(());
    };

    let current_len = file.metadata()?.len();
    if current_len < journal.previous_len {
        return Err(ExecutionLedgerError::InvalidRecord(
            "ledger is shorter than append journal predecessor".to_owned(),
        ));
    }
    if sha256_prefix(file, journal.previous_len)? != journal.previous_sha256 {
        return Err(ExecutionLedgerError::InvalidRecord(
            "ledger predecessor does not match append journal".to_owned(),
        ));
    }
    let mut prefix = file.try_clone()?;
    prefix.seek(SeekFrom::Start(0))?;
    let (mut previous_events, tail) =
        read_complete_events_and_tail(prefix.take(journal.previous_len))?;
    if !tail.is_empty() {
        return Err(ExecutionLedgerError::InvalidRecord(
            "append journal predecessor is not newline-terminated".to_owned(),
        ));
    }
    if !verify_events(&previous_events)? {
        return Err(ExecutionLedgerError::InvalidChain(
            "append journal predecessor chain is invalid".to_owned(),
        ));
    }
    previous_events.push(parse_canonical_event(&journal.record)?);
    if !verify_events(&previous_events)? {
        return Err(ExecutionLedgerError::InvalidChain(
            "prepared append violates execution lifecycle".to_owned(),
        ));
    }
    let mut expected = journal.record.into_bytes();
    expected.push(b'\n');
    let suffix_len = current_len - journal.previous_len;
    let expected_len = u64::try_from(expected.len()).map_err(|_| {
        ExecutionLedgerError::InvalidRecord("prepared append exceeds platform".to_owned())
    })?;
    if suffix_len > expected_len {
        return Err(ExecutionLedgerError::InvalidRecord(
            "ledger contains bytes after prepared append".to_owned(),
        ));
    }
    let suffix_len = usize::try_from(suffix_len).map_err(|_| {
        ExecutionLedgerError::InvalidRecord("prepared ledger suffix exceeds platform".to_owned())
    })?;
    let mut suffix = vec![0; suffix_len];
    let mut suffix_reader = file.try_clone()?;
    suffix_reader.seek(SeekFrom::Start(journal.previous_len))?;
    suffix_reader.read_exact(&mut suffix)?;
    if !expected.starts_with(&suffix) {
        return Err(ExecutionLedgerError::InvalidRecord(
            "ledger tail does not match prepared append".to_owned(),
        ));
    }
    if suffix_len < expected.len() {
        file.seek(SeekFrom::End(0))?;
        file.write_all(&expected[suffix_len..])?;
    }
    file.flush()?;
    file.sync_data()?;
    operation.sync_parent()?;
    validate_operation_file(
        operation,
        &operation.relative,
        file,
        "execution ledger",
        true,
    )?;
    if !verify_events(&read_events_from_file(file)?)? {
        return Err(ExecutionLedgerError::InvalidChain(
            "completed prepared append is invalid".to_owned(),
        ));
    }
    clear_append_journal_operation(operation)
}

#[cfg(any(unix, test))]
fn file_is_newline_terminated(file: &File) -> Result<bool> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0];
    reader.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

#[cfg(any(unix, test))]
fn file_len_and_sha256(file: &File) -> Result<(u64, String)> {
    let len = file.metadata()?.len();
    Ok((len, sha256_prefix(file, len)?))
}

#[cfg(any(unix, test))]
fn sha256_prefix(file: &File, len: u64) -> Result<String> {
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut remaining = len;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    while remaining > 0 {
        let limit = usize::try_from(remaining)
            .unwrap_or(buffer.len())
            .min(buffer.len());
        let read = reader.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(ExecutionLedgerError::InvalidRecord(
                "ledger ended before append journal predecessor".to_owned(),
            ));
        }
        digest.update(&buffer[..read]);
        remaining -= u64::try_from(read).map_err(|_| {
            ExecutionLedgerError::InvalidRecord("ledger read length overflow".to_owned())
        })?;
    }
    Ok(encode_sha256(digest.finalize()))
}

#[cfg(test)]
fn append_journal_path(path: &Path) -> PathBuf {
    let mut journal = path.as_os_str().to_os_string();
    journal.push(".append-journal");
    PathBuf::from(journal)
}

#[cfg(test)]
fn append_journal_temp_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".append-journal.tmp");
    PathBuf::from(temporary)
}

#[cfg(test)]
fn write_append_journal(
    path: &Path,
    previous_len: u64,
    previous_sha256: &str,
    record: &str,
) -> Result<()> {
    let (root, relative) = path_parts(path)?;
    #[cfg(unix)]
    {
        let root_identity = capture_directory_identity(&root)?;
        let parent_identity = capture_relative_parent_identity(&root, &relative)?;
        let operation = unix_open_operation(
            &root,
            &relative,
            Some(root_identity),
            parent_identity,
            false,
        )?;
        write_append_journal_operation(&operation, previous_len, previous_sha256, record)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, relative, previous_len, previous_sha256, record);
        Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger descriptor-relative access is unsupported on this platform"
                .to_owned(),
        ))
    }
}

#[cfg(unix)]
fn write_append_journal_operation(
    operation: &Operation,
    previous_len: u64,
    previous_sha256: &str,
    record: &str,
) -> Result<()> {
    if record.len() > MAX_LEDGER_RECORD_BYTES {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger record exceeds byte limit".to_owned(),
        ));
    }
    let journal = AppendJournalV1 {
        schema: APPEND_JOURNAL_SCHEMA.to_owned(),
        previous_len,
        previous_sha256: previous_sha256.to_owned(),
        record: record.to_owned(),
        record_sha256: sha256(record.as_bytes()),
    };
    let bytes = serde_json::to_vec(&journal)?;
    if bytes.len() > MAX_APPEND_JOURNAL_BYTES {
        return Err(ExecutionLedgerError::InvalidRecord(
            "append journal exceeds byte limit".to_owned(),
        ));
    }
    let temporary = relative_sibling(&operation.relative, ".append-journal.tmp")?;
    let journal = relative_sibling(&operation.relative, ".append-journal")?;
    if remove_file_operation(operation, &temporary)? {
        operation.sync_parent()?;
    }
    let mut file = create_regular_operation(operation, &temporary, "append journal temporary")?;
    let mut published = false;
    let result = (|| {
        operation.validate_paths()?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
        validate_operation_file(
            operation,
            &temporary,
            &file,
            "append journal temporary",
            true,
        )?;
        hard_link_operation(operation, &temporary, &journal)?;
        published = true;
        operation.sync_parent()?;
        let journal_file = open_nofollow_operation(operation, &journal)?.ok_or_else(|| {
            ExecutionLedgerError::InvalidRecord("published append journal disappeared".to_owned())
        })?;
        validate_journal_link_pair_operation(
            operation,
            &journal,
            &temporary,
            &journal_file,
            &file,
        )?;
        unlink_operation(operation, &temporary)?;
        operation.sync_parent()
    })();
    if result.is_err() {
        if published {
            let _ = clear_append_journal_operation(operation);
        } else {
            let _ = remove_file_operation(operation, &temporary);
        }
    }
    result
}

#[cfg(any(unix, test))]
fn read_bounded_file(mut file: File, max_bytes: usize, label: &str) -> Result<Vec<u8>> {
    let max_len = u64::try_from(max_bytes).map_err(|_| {
        ExecutionLedgerError::InvalidRecord(format!("{label} byte limit exceeds platform"))
    })?;
    if file.metadata()?.len() > max_len {
        return Err(ExecutionLedgerError::InvalidRecord(format!(
            "{label} exceeds byte limit"
        )));
    }
    let capacity = usize::try_from(file.metadata()?.len()).unwrap_or(max_bytes);
    let mut bytes = Vec::with_capacity(capacity.min(max_bytes));
    std::io::Read::by_ref(&mut file)
        .take(max_len.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(ExecutionLedgerError::InvalidRecord(format!(
            "{label} exceeds byte limit"
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn validate_journal_link_pair_operation(
    operation: &Operation,
    journal_path: &Path,
    temporary_path: &Path,
    journal: &File,
    temporary: &File,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let journal_metadata = journal.metadata()?;
    let temporary_metadata = temporary.metadata()?;
    let same_inode = journal_metadata.dev() == temporary_metadata.dev()
        && journal_metadata.ino() == temporary_metadata.ino();
    if !journal_metadata.is_file()
        || !temporary_metadata.is_file()
        || journal_metadata.nlink() != 2
        || temporary_metadata.nlink() != 2
        || !same_inode
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "append journal link pair identity is invalid".to_owned(),
        ));
    }
    let journal_stat = operation_stat(operation, journal_path)?;
    let temporary_stat = operation_stat(operation, temporary_path)?;
    if journal_stat.st_mode as libc::mode_t & libc::S_IFMT != libc::S_IFREG
        || temporary_stat.st_mode as libc::mode_t & libc::S_IFMT != libc::S_IFREG
        || journal_stat.st_nlink as u64 != 2
        || temporary_stat.st_nlink as u64 != 2
        || journal_stat.st_dev as u64 != journal_metadata.dev()
        || journal_stat.st_ino as u64 != journal_metadata.ino()
        || temporary_stat.st_dev as u64 != journal_metadata.dev()
        || temporary_stat.st_ino as u64 != journal_metadata.ino()
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "append journal link pair changed while reading".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn read_append_journal_operation(operation: &Operation) -> Result<Option<AppendJournalV1>> {
    let journal = relative_sibling(&operation.relative, ".append-journal")?;
    let temporary = relative_sibling(&operation.relative, ".append-journal.tmp")?;
    let Some(file) = open_nofollow_operation(operation, &journal)? else {
        return Ok(None);
    };
    let bytes = read_bounded_file(
        file.try_clone()?,
        MAX_APPEND_JOURNAL_BYTES,
        "append journal",
    )?;
    use std::os::unix::fs::MetadataExt;
    if file.metadata()?.nlink() == 2 {
        let Some(temp_file) = open_nofollow_operation(operation, &temporary)? else {
            return Err(ExecutionLedgerError::InvalidRecord(
                "append journal link pair is incomplete".to_owned(),
            ));
        };
        let temp_bytes = read_bounded_file(
            temp_file.try_clone()?,
            MAX_APPEND_JOURNAL_BYTES,
            "append journal temporary",
        )?;
        if bytes != temp_bytes {
            return Err(ExecutionLedgerError::InvalidRecord(
                "append journal link pair content differs".to_owned(),
            ));
        }
        validate_journal_link_pair_operation(operation, &journal, &temporary, &file, &temp_file)?;
    } else {
        validate_operation_file(operation, &journal, &file, "append journal", true)?;
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        ExecutionLedgerError::InvalidRecord("append journal is not UTF-8".to_owned())
    })?;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = StrictJson::deserialize(&mut deserializer)
        .map_err(serde_json::Error::custom)?
        .into_value();
    deserializer.end()?;
    let journal: AppendJournalV1 = serde_json::from_value(value)?;
    if journal.schema != APPEND_JOURNAL_SCHEMA
        || serde_json::to_vec(&journal)? != bytes
        || journal.record.len() > MAX_LEDGER_RECORD_BYTES
        || sha256(journal.record.as_bytes()) != journal.record_sha256
        || parse_canonical_event(&journal.record).is_err()
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "append journal is invalid or non-canonical".to_owned(),
        ));
    }
    Ok(Some(journal))
}

#[cfg(unix)]
fn clear_append_journal_operation(operation: &Operation) -> Result<()> {
    let journal = relative_sibling(&operation.relative, ".append-journal")?;
    let temporary = relative_sibling(&operation.relative, ".append-journal.tmp")?;
    if let Some(journal_file) = open_nofollow_operation(operation, &journal)? {
        use std::os::unix::fs::MetadataExt;
        if journal_file.metadata()?.nlink() == 2 {
            let temporary_file =
                open_nofollow_operation(operation, &temporary)?.ok_or_else(|| {
                    ExecutionLedgerError::InvalidRecord(
                        "append journal link pair is incomplete".to_owned(),
                    )
                })?;
            validate_journal_link_pair_operation(
                operation,
                &journal,
                &temporary,
                &journal_file,
                &temporary_file,
            )?;
            unlink_operation(operation, &journal)?;
            unlink_operation(operation, &temporary)?;
        } else {
            remove_file_operation(operation, &journal)?;
            let _ = remove_file_operation(operation, &temporary)?;
        }
    } else {
        let _ = remove_file_operation(operation, &temporary)?;
    }
    operation.sync_parent()
}

#[cfg(any(unix, test))]
fn sha256(bytes: &[u8]) -> String {
    encode_sha256(Sha256::digest(bytes))
}

#[cfg(any(unix, test))]
fn encode_sha256(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest.as_ref() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(any(unix, test))]
enum StrictJson {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Self>),
    Object(serde_json::Map<String, Value>),
}

#[cfg(any(unix, test))]
impl StrictJson {
    fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
            Self::Array(values) => Value::Array(values.into_iter().map(Self::into_value).collect()),
            Self::Object(value) => Value::Object(value),
        }
    }
}

#[cfg(any(unix, test))]
impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrictVisitor;

        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictJson;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("strict JSON without duplicate keys or floats")
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::Null)
            }

            fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::Null)
            }

            fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::Number(Number::from(value)))
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::Number(Number::from(value)))
            }

            fn visit_f64<E>(self, _value: f64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Err(E::custom("floating-point ledger numbers are forbidden"))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::String(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson::String(value))
            }

            fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictJson>()? {
                    values.push(value);
                }
                Ok(StrictJson::Array(values))
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!(
                            "duplicate ledger JSON key '{key}'"
                        )));
                    }
                    values.insert(key, map.next_value::<StrictJson>()?.into_value());
                }
                Ok(StrictJson::Object(values))
            }
        }

        deserializer.deserialize_any(StrictVisitor)
    }
}

#[cfg(any(unix, test))]
fn parse_canonical_event(line: &str) -> Result<ExecutionEvent> {
    let mut deserializer = serde_json::Deserializer::from_str(line);
    let value = StrictJson::deserialize(&mut deserializer)
        .map_err(serde_json::Error::custom)?
        .into_value();
    deserializer.end()?;
    let record: LedgerRecordV1 = serde_json::from_value(value)?;
    if record.schema != LEDGER_RECORD_SCHEMA || record.kind != LEDGER_RECORD_KIND {
        return Err(ExecutionLedgerError::InvalidRecord(
            "unsupported execution ledger record schema or kind".to_owned(),
        ));
    }
    if serde_json::to_string(&record)? != line {
        return Err(ExecutionLedgerError::InvalidRecord(
            "execution ledger record is not canonical JSON".to_owned(),
        ));
    }
    if hash_event(&record.event)? != record.entry_hash {
        return Err(ExecutionLedgerError::InvalidChain(
            "execution ledger record digest mismatch".to_owned(),
        ));
    }
    Ok(record.event)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_started() -> ExecutionEvent {
        ExecutionEvent::TaskStarted {
            task_id: "task-1".to_owned(),
            trace_id: "trace-1".to_owned(),
            envelope_ref: "task:1".to_owned(),
            timestamp: "2026-08-23T12:00:00Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn plan_created() -> ExecutionEvent {
        ExecutionEvent::PlanCreated {
            task_id: "task-1".to_owned(),
            trace_id: "trace-1".to_owned(),
            plan_id: "plan-1".to_owned(),
            plan_ref: "plan:1".to_owned(),
            timestamp: "2026-08-23T12:00:01Z".to_owned(),
            sequence_number: 0,
            prev_hash: String::new(),
        }
    }

    fn stage_prepared_append(
        store: &ExecutionLedgerStore,
        event: ExecutionEvent,
        omitted_suffix: usize,
    ) -> Vec<u8> {
        let previous = fs::read(store.path()).unwrap_or_default();
        store.append(event).unwrap();
        let complete = fs::read(store.path()).unwrap();
        let record = complete[previous.len()..].to_vec();
        assert!(record.ends_with(b"\n"));
        OpenOptions::new()
            .write(true)
            .open(store.path())
            .unwrap()
            .set_len(u64::try_from(previous.len()).unwrap())
            .unwrap();
        let line = std::str::from_utf8(&record[..record.len() - 1]).unwrap();
        write_append_journal(
            store.path(),
            u64::try_from(previous.len()).unwrap(),
            &sha256(&previous),
            line,
        )
        .unwrap();
        let written = record.len().saturating_sub(omitted_suffix);
        let mut file = OpenOptions::new().append(true).open(store.path()).unwrap();
        file.write_all(&record[..written]).unwrap();
        file.sync_all().unwrap();
        record
    }

    #[test]
    fn prepared_complete_record_without_newline_is_finalized_before_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        stage_prepared_append(&store, task_started(), 1);

        assert!(store.load_verified().is_err());
        assert!(!store.append_if_new(task_started()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 1);
        assert!(fs::read(&path).unwrap().ends_with(b"\n"));
        assert!(!append_journal_path(&path).exists());
    }

    #[test]
    fn prepared_partial_record_is_completed_before_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 7);

        assert!(!store.append_if_new(plan_created()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 2);
        assert!(store.verify_chain().unwrap());
        assert!(!append_journal_path(&path).exists());
    }

    #[test]
    fn prepared_record_is_completed_when_no_ledger_byte_was_written() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), usize::MAX);

        assert!(!store.append_if_new(plan_created()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 2);
    }

    #[test]
    fn prepared_first_record_is_completed_from_empty_ledger() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        stage_prepared_append(&store, task_started(), usize::MAX);

        assert!(!store.append_if_new(task_started()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 1);
    }

    #[test]
    fn completed_prepared_record_only_clears_stale_journal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 0);

        assert!(store.load_verified().is_err());
        assert!(!store.append_if_new(plan_created()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 2);
        assert!(!append_journal_path(&path).exists());
    }

    #[test]
    fn newline_terminated_corruption_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"garbage\n").unwrap();
        file.sync_all().unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert!(store.by_task_verified("task-1").is_err());
        assert!(store.last_sequence_verified().is_err());
        assert!(store.canonical_receipt_for_task_verified("task-1").is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn unmatched_unterminated_corruption_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"garbage").unwrap();
        file.sync_all().unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn tampered_prefix_prevents_prepared_tail_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 7);
        let bytes = fs::read(&path).unwrap();
        let tampered = String::from_utf8(bytes)
            .unwrap()
            .replace("task-1", "task-X");
        fs::write(&path, tampered.as_bytes()).unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn prepared_tail_mismatch_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 7);
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&path, &bytes).unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn bytes_after_prepared_record_remain_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 0);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"extra").unwrap();
        file.sync_all().unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn prepared_lifecycle_violation_is_rejected_before_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let previous = fs::read(&path).unwrap();
        let previous_event = store.load_verified().unwrap().pop().unwrap();
        let mut event = task_started();
        event.set_chain_fields(2, hash_event(&previous_event).unwrap());
        let entry_hash = hash_event(&event).unwrap();
        event.set_entry_hash(entry_hash.clone());
        let line = serde_json::to_string(&LedgerRecordV1 {
            schema: LEDGER_RECORD_SCHEMA.to_owned(),
            kind: LEDGER_RECORD_KIND.to_owned(),
            event,
            entry_hash,
        })
        .unwrap();
        write_append_journal(
            &path,
            u64::try_from(previous.len()).unwrap(),
            &sha256(&previous),
            &line,
        )
        .unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
    }

    #[test]
    fn crlf_terminated_record_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes.pop();
        bytes.extend_from_slice(b"\r\n");
        fs::write(&path, &bytes).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn incomplete_utf8_tail_without_journal_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0xe2, 0x82]).unwrap();
        file.sync_all().unwrap();

        let before = fs::read(&path).unwrap();
        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn incomplete_first_record_without_journal_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        fs::write(&path, b"{\"schema\":").unwrap();
        let store = ExecutionLedgerStore::new(&path);
        let before = fs::read(&path).unwrap();

        assert!(store.append(task_started()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn valid_unterminated_record_without_journal_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(file.metadata().unwrap().len() - 1).unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.load_verified().is_err());
        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn every_pending_journal_state_is_hidden_until_exclusive_recovery() {
        for omitted_suffix in [usize::MAX, 7, 1, 0] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("ledger.jsonl");
            let store = ExecutionLedgerStore::new(&path);
            stage_prepared_append(&store, task_started(), omitted_suffix);

            assert!(store.load().is_err());
            assert!(store.load_verified().is_err());
            assert!(store.verify_chain().is_err());
            assert!(store.by_task_verified("task-1").is_err());
            assert!(store.last_sequence_verified().is_err());
            assert!(store.canonical_receipt_for_task_verified("task-1").is_err());
            assert!(!store.append_if_new(task_started()).unwrap());
            assert_eq!(store.load_verified().unwrap().len(), 1);
            assert_eq!(store.by_task_verified("task-1").unwrap().len(), 1);
            assert_eq!(store.last_sequence_verified().unwrap(), 1);
        }
    }

    #[test]
    fn pending_journal_without_ledger_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        fs::write(append_journal_path(&path), b"{}").unwrap();
        let store = ExecutionLedgerStore::new(&path);

        assert!(store.load().is_err());
        assert!(store.load_verified().is_err());
        assert!(store.verify_chain().is_err());
        assert!(store.append(task_started()).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn temporary_journal_is_hidden_then_cleaned_by_next_append() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        fs::write(append_journal_temp_path(&path), b"stale prepared bytes").unwrap();

        assert!(store.load_verified().is_err());
        assert!(!store.append_if_new(task_started()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 1);
        assert!(!append_journal_temp_path(&path).exists());
    }

    #[test]
    fn oversized_journal_is_rejected_without_ledger_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let before = fs::read(&path).unwrap();
        fs::write(
            append_journal_path(&path),
            vec![b' '; MAX_APPEND_JOURNAL_BYTES + 1],
        )
        .unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn journal_symlink_is_rejected_without_touching_target_or_ledger() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let target = directory.path().join("target");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let before = fs::read(&path).unwrap();
        fs::write(&target, b"target-bytes").unwrap();
        symlink(&target, append_journal_path(&path)).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(&target).unwrap(), b"target-bytes");
    }

    #[test]
    fn journal_publish_never_replaces_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        fs::write(append_journal_path(&path), b"existing").unwrap();

        assert!(write_append_journal(&path, 0, &sha256(b""), "{}").is_err());
        assert_eq!(fs::read(append_journal_path(&path)).unwrap(), b"existing");
    }

    #[cfg(unix)]
    #[test]
    fn crash_after_journal_link_before_temp_unlink_recovers_prepared_state() {
        use std::fs::hard_link;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 7);
        let journal = append_journal_path(&path);
        let temporary = append_journal_temp_path(&path);
        hard_link(&journal, &temporary).unwrap();

        assert!(store.load_verified().is_err());
        assert!(!store.append_if_new(plan_created()).unwrap());
        assert_eq!(store.load_verified().unwrap().len(), 2);
        assert!(!journal.exists());
        assert!(!temporary.exists());
    }

    #[test]
    fn journal_error_cleans_unpublished_temporary_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let journal = append_journal_path(&path);
        let temporary = append_journal_temp_path(&path);
        fs::write(&journal, b"existing").unwrap();

        assert!(write_append_journal(&path, 0, &sha256(b""), "{}").is_err());
        assert_eq!(fs::read(&journal).unwrap(), b"existing");
        assert!(!temporary.exists());
    }

    #[test]
    fn oversized_record_is_rejected_before_journal_or_ledger_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        let mut event = task_started();
        if let ExecutionEvent::TaskStarted { envelope_ref, .. } = &mut event {
            *envelope_ref = "x".repeat(MAX_LEDGER_RECORD_BYTES + 1);
        }

        assert!(store.append(event).is_err());
        assert_eq!(fs::read(&path).unwrap(), Vec::<u8>::new());
        assert!(!append_journal_path(&path).exists());
        assert!(!append_journal_temp_path(&path).exists());
    }

    #[test]
    fn oversized_prepared_record_is_rejected_before_recovery_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        fs::write(&path, b"").unwrap();
        let store = ExecutionLedgerStore::new(&path);
        let mut event = task_started();
        if let ExecutionEvent::TaskStarted { envelope_ref, .. } = &mut event {
            *envelope_ref = "x".repeat(MAX_LEDGER_RECORD_BYTES + 1);
        }
        event.set_chain_fields(1, GENESIS.to_owned());
        let entry_hash = hash_event(&event).unwrap();
        event.set_entry_hash(entry_hash.clone());
        let record = serde_json::to_string(&LedgerRecordV1 {
            schema: LEDGER_RECORD_SCHEMA.to_owned(),
            kind: LEDGER_RECORD_KIND.to_owned(),
            event,
            entry_hash,
        })
        .unwrap();
        let journal = AppendJournalV1 {
            schema: APPEND_JOURNAL_SCHEMA.to_owned(),
            previous_len: 0,
            previous_sha256: sha256(b""),
            record_sha256: sha256(record.as_bytes()),
            record,
        };
        fs::write(
            append_journal_path(&path),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();

        assert!(store.append(task_started()).is_err());
        assert_eq!(fs::read(&path).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn oversized_on_disk_record_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        fs::write(&path, vec![b'x'; MAX_LEDGER_RECORD_BYTES + 2]).unwrap();
        let store = ExecutionLedgerStore::new(&path);

        assert!(store.load().is_err());
    }

    #[test]
    fn corrupt_append_journal_remains_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        stage_prepared_append(&store, plan_created(), 7);
        fs::write(append_journal_path(&path), b"{\"schema\":").unwrap();
        let before = fs::read(&path).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn ledger_symlink_is_rejected_by_every_read_projection() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.jsonl");
        let path = directory.path().join("ledger.jsonl");
        let target_store = ExecutionLedgerStore::new(&target);
        target_store.append(task_started()).unwrap();
        symlink(&target, &path).unwrap();
        let store = ExecutionLedgerStore::new(&path);

        assert!(store.load().is_err());
        assert!(store.load_verified().is_err());
        assert!(store.verify_chain().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn parent_symlink_is_rejected_before_ledger_open() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target_directory = tempfile::tempdir().unwrap();
        let target = target_directory.path().join("ledger.jsonl");
        let target_store = ExecutionLedgerStore::new(&target);
        target_store.append(task_started()).unwrap();
        let parent = directory.path().join("execution");
        symlink(target_directory.path(), &parent).unwrap();
        let store = ExecutionLedgerStore::new(parent.join("ledger.jsonl"));

        assert!(store.load().is_err());
        assert!(store.load_verified().is_err());
        assert!(store.verify_chain().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn hardlink_alias_is_rejected_after_descriptor_identity_check() {
        use std::fs::hard_link;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let alias = directory.path().join("ledger-alias.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        hard_link(&path, &alias).unwrap();

        assert!(store.load().is_err());
        assert!(store.load_verified().is_err());
        assert!(store.verify_chain().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn opened_descriptor_is_not_redirected_by_path_swap() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let moved = directory.path().join("ledger-moved.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let (_, relative) = path_parts(&path).unwrap();
        let operation = store.open_operation(false).unwrap();
        let file = open_regular_nofollow_operation(&operation, &relative)
            .unwrap()
            .unwrap();

        fs::rename(&path, &moved).unwrap();
        assert!(read_events_from_file(&file).is_ok());
        assert!(
            validate_operation_file(&operation, &relative, &file, "execution ledger", true)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn opened_descriptor_rejects_parent_swap_after_acquisition() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("execution");
        fs::create_dir(&parent).unwrap();
        let path = parent.join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let (_, relative) = path_parts(&path).unwrap();
        let operation = store.open_operation(false).unwrap();
        let file = open_regular_nofollow_operation(&operation, &relative)
            .unwrap()
            .unwrap();

        let moved = directory.path().join("execution-moved");
        fs::rename(&parent, &moved).unwrap();
        symlink(&moved, &parent).unwrap();
        assert!(read_events_from_file(&file).is_ok());
        assert!(
            validate_operation_file(&operation, &relative, &file, "execution ledger", true)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn hardlink_created_after_open_is_rejected_before_commit() {
        use std::fs::hard_link;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let alias = directory.path().join("ledger-race-alias.jsonl");
        let store = ExecutionLedgerStore::new(&path);
        store.append(task_started()).unwrap();
        let operation = store.open_operation(false).unwrap();
        let file = open_regular_nofollow_operation(&operation, Path::new("ledger.jsonl"))
            .unwrap()
            .unwrap();
        hard_link(&path, &alias).unwrap();

        assert!(
            validate_operation_file(
                &operation,
                Path::new("ledger.jsonl"),
                &file,
                "execution ledger",
                true,
            )
            .is_err()
        );
        assert!(store.append(plan_created()).is_err());
    }

    #[test]
    fn verified_constructor_rejects_parent_traversal() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            ExecutionLedgerStore::new_verified(
                directory.path(),
                Path::new("nested/../ledger.jsonl")
            )
            .is_err()
        );
        assert!(
            ExecutionLedgerStore::new_verified(
                directory.path(),
                Path::new("/outside/ledger.jsonl")
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn verified_constructor_rejects_symlink_root() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let real_root = directory.path().join("real-root");
        let alias_root = directory.path().join("alias-root");
        fs::create_dir(&real_root).unwrap();
        symlink(&real_root, &alias_root).unwrap();

        assert!(ExecutionLedgerStore::new_verified(&alias_root, "ledger.jsonl").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn verified_root_replacement_is_rejected_before_append_side_effect() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let moved = directory.path().join("root-moved");
        fs::create_dir(&root).unwrap();
        let store = ExecutionLedgerStore::new_verified(&root, "ledger.jsonl").unwrap();

        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();

        assert!(store.append(task_started()).is_err());
        assert!(!root.join("ledger.jsonl").exists());
        assert!(!moved.join("ledger.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn verified_parent_replacement_is_rejected_before_append_side_effect() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let parent = root.join("execution");
        let moved = root.join("execution-moved");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&parent).unwrap();
        let store = ExecutionLedgerStore::new_verified(&root, "execution/ledger.jsonl").unwrap();

        fs::rename(&parent, &moved).unwrap();
        fs::create_dir(&parent).unwrap();

        assert!(store.append(task_started()).is_err());
        assert!(!parent.join("ledger.jsonl").exists());
        assert!(!moved.join("ledger.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn created_parent_identity_is_pinned_for_later_operations() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let parent = root.join("execution");
        let moved = root.join("execution-moved");
        fs::create_dir(&root).unwrap();
        let store = ExecutionLedgerStore::new_verified(&root, "execution/ledger.jsonl").unwrap();

        store.append(task_started()).unwrap();
        let before = fs::read(parent.join("ledger.jsonl")).unwrap();
        fs::rename(&parent, &moved).unwrap();
        fs::create_dir(&parent).unwrap();

        assert!(store.append(plan_created()).is_err());
        assert!(!parent.join("ledger.jsonl").exists());
        assert_eq!(fs::read(moved.join("ledger.jsonl")).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn default_data_root_creation_is_descriptor_relative_and_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("missing").join("data");

        ensure_default_data_root(&root).unwrap();

        assert!(root.is_dir());
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn legacy_relative_constructor_resolves_current_directory() {
        let store = ExecutionLedgerStore::new(Path::new("ledger.jsonl"));
        assert!(store.path().is_absolute());
        assert_eq!(
            store.path().file_name(),
            Some(std::ffi::OsStr::new("ledger.jsonl"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_ledger_fails_closed_before_reparse_or_path_side_effect() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let store = ExecutionLedgerStore::new(&path);

        assert!(store.append(task_started()).is_err());
        assert!(!path.exists());
    }
}
