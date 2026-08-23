//! File-backed append-only execution ledger.
//! The append journal proves local crash continuity; signed canonical receipts,
//! not this unkeyed sidecar, provide adversarial authenticity.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::de::Error as _;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

use super::event::ExecutionEvent;
use super::verify::{GENESIS, hash_event, verify_events};
use super::{ExecutionLedgerError, Result};

const LEDGER_RECORD_SCHEMA: &str = "lean-ctx.execution-ledger-record.v1";
const LEDGER_RECORD_KIND: &str = "execution_event";
const APPEND_JOURNAL_SCHEMA: &str = "lean-ctx.execution-ledger-append-journal.v1";
const MAX_LEDGER_RECORD_BYTES: usize = 1024 * 1024;
const MAX_APPEND_JOURNAL_BYTES: usize = MAX_LEDGER_RECORD_BYTES * 2 + 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LedgerRecordV1 {
    schema: String,
    kind: String,
    event: ExecutionEvent,
    entry_hash: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AppendJournalV1 {
    schema: String,
    previous_len: u64,
    previous_sha256: String,
    record: String,
    record_sha256: String,
}

/// Default execution-ledger location: `<data_dir>/execution/ledger.jsonl`.
pub fn default_path() -> Option<PathBuf> {
    let data_dir = crate::core::data_dir::lean_ctx_data_dir().ok()?;
    let directory = data_dir.join("execution");
    fs::create_dir_all(&directory).ok()?;
    Some(directory.join("ledger.jsonl"))
}

/// A process-independent handle to an execution ledger file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionLedgerStore {
    path: PathBuf,
}

impl ExecutionLedgerStore {
    /// Creates a store backed by `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Creates a store at the configured default location.
    pub fn from_default() -> Result<Self> {
        default_path().map(Self::new).ok_or_else(|| {
            ExecutionLedgerError::InvalidRecord("data directory unavailable".to_owned())
        })
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
    pub(crate) fn append_if_new(&self, mut event: ExecutionEvent) -> Result<bool> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = open_ledger_for_append(&self.path)?;
        file.lock_exclusive()?;

        let result = (|| {
            recover_pending_append(&mut file, &self.path)?;
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
            write_append_journal(&self.path, previous_len, &previous_sha256, &line)?;
            file.seek(SeekFrom::End(0))?;
            writeln!(file, "{line}")?;
            file.flush()?;
            file.sync_data()?;
            sync_parent_directory(&self.path)?;
            clear_append_journal(&self.path)?;
            Ok(true)
        })();

        let _ = FileExt::unlock(&file);
        result
    }

    /// Verifies the complete on-disk chain under a shared file lock.
    pub fn verify_chain(&self) -> Result<bool> {
        ensure_no_pending_append(&self.path)?;
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.into()),
        };
        file.lock_shared()?;
        let result = match ensure_no_pending_append(&self.path)
            .and_then(|()| read_events_from_file(&file))
        {
            Ok(events) => verify_events(&events),
            Err(ExecutionLedgerError::InvalidChain(_)) => Ok(false),
            Err(error) => Err(error),
        };
        let _ = FileExt::unlock(&file);
        result
    }

    /// Loads all well-formed events in file order.
    pub fn load(&self) -> Result<Vec<ExecutionEvent>> {
        ensure_no_pending_append(&self.path)?;
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        file.lock_shared()?;
        let result =
            ensure_no_pending_append(&self.path).and_then(|()| read_events_from_file(&file));
        let _ = FileExt::unlock(&file);
        result
    }

    /// Loads one snapshot only when its record digests, chain, and lifecycle verify.
    pub fn load_verified(&self) -> Result<Vec<ExecutionEvent>> {
        ensure_no_pending_append(&self.path)?;
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        file.lock_shared()?;
        let result = ensure_no_pending_append(&self.path)
            .and_then(|()| read_events_from_file(&file))
            .and_then(|events| {
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

fn open_ledger_for_append(path: &Path) -> Result<File> {
    loop {
        let mut existing = OpenOptions::new();
        existing.read(true).append(true);
        apply_nofollow(&mut existing);
        match existing.open(path) {
            Ok(file) if file.metadata()?.is_file() => return Ok(file),
            Ok(_) => {
                return Err(ExecutionLedgerError::InvalidRecord(
                    "execution ledger path is not a regular file".to_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        ensure_no_pending_append(path)?;
        let mut create = OpenOptions::new();
        create.read(true).append(true).create_new(true);
        apply_nofollow(&mut create);
        match create.open(path) {
            Ok(file) => {
                sync_parent_directory(path)?;
                return Ok(file);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(unix)]
fn apply_nofollow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
}

#[cfg(not(unix))]
fn apply_nofollow(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
    Ok(())
}

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

fn ensure_no_pending_append(path: &Path) -> Result<()> {
    for pending in [append_journal_path(path), append_journal_temp_path(path)] {
        match fs::symlink_metadata(pending) {
            Ok(_) => {
                return Err(ExecutionLedgerError::InvalidRecord(
                    "execution ledger has a pending append journal".to_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

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

fn recover_pending_append(file: &mut File, path: &Path) -> Result<()> {
    let Some(journal) = read_append_journal(path)? else {
        if !file_is_newline_terminated(file)? {
            return Err(ExecutionLedgerError::InvalidRecord(
                "unterminated ledger tail has no matching append journal".to_owned(),
            ));
        }
        if remove_file_if_exists(&append_journal_temp_path(path))? {
            sync_parent_directory(path)?;
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
    sync_parent_directory(path)?;
    if !verify_events(&read_events_from_file(file)?)? {
        return Err(ExecutionLedgerError::InvalidChain(
            "completed prepared append is invalid".to_owned(),
        ));
    }
    clear_append_journal(path)
}

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

fn file_len_and_sha256(file: &File) -> Result<(u64, String)> {
    let len = file.metadata()?.len();
    Ok((len, sha256_prefix(file, len)?))
}

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

fn append_journal_path(path: &Path) -> PathBuf {
    let mut journal = path.as_os_str().to_os_string();
    journal.push(".append-journal");
    PathBuf::from(journal)
}

fn append_journal_temp_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".append-journal.tmp");
    PathBuf::from(temporary)
}

fn write_append_journal(
    path: &Path,
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
    let temporary = append_journal_temp_path(path);
    match fs::remove_file(&temporary) {
        Ok(()) => sync_parent_directory(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    let journal = append_journal_path(path);
    fs::hard_link(&temporary, &journal)?;
    sync_parent_directory(path)?;
    fs::remove_file(&temporary)?;
    sync_parent_directory(path)
}

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

fn read_append_journal(path: &Path) -> Result<Option<AppendJournalV1>> {
    let journal_path = append_journal_path(path);
    let Some(file) = open_regular_nofollow(&journal_path)? else {
        return Ok(None);
    };
    let bytes = read_bounded_file(file, MAX_APPEND_JOURNAL_BYTES, "append journal")?;
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

fn clear_append_journal(path: &Path) -> Result<()> {
    fs::remove_file(append_journal_path(path))?;
    remove_file_if_exists(&append_journal_temp_path(path))?;
    sync_parent_directory(path)
}

fn remove_file_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn open_regular_nofollow(path: &Path) -> Result<Option<File>> {
    #[cfg(not(unix))]
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(ExecutionLedgerError::InvalidRecord(
            "append journal is not a regular file".to_owned(),
        ));
    }

    let mut options = OpenOptions::new();
    options.read(true);
    apply_nofollow(&mut options);
    match options.open(path) {
        Ok(file) if file.metadata()?.is_file() => Ok(Some(file)),
        Ok(_) => Err(ExecutionLedgerError::InvalidRecord(
            "append journal is not a regular file".to_owned(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn sha256(bytes: &[u8]) -> String {
    encode_sha256(Sha256::digest(bytes))
}

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

enum StrictJson {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Self>),
    Object(serde_json::Map<String, Value>),
}

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
}
