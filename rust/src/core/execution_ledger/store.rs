//! File-backed append-only execution ledger.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::de::Error as _;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use super::event::ExecutionEvent;
use super::verify::{GENESIS, hash_event, verify_events};
use super::{ExecutionLedgerError, Result};

const LEDGER_RECORD_SCHEMA: &str = "lean-ctx.execution-ledger-record.v1";
const LEDGER_RECORD_KIND: &str = "execution_event";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LedgerRecordV1 {
    schema: String,
    kind: String,
    event: ExecutionEvent,
    entry_hash: String,
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

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.path)?;
        file.lock_exclusive()?;

        let result = (|| {
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
            file.seek(SeekFrom::End(0))?;
            writeln!(file, "{line}")?;
            file.flush()?;
            file.sync_data()?;
            sync_parent_directory(&self.path)?;
            Ok(true)
        })();

        let _ = FileExt::unlock(&file);
        result
    }

    /// Verifies the complete on-disk chain under a shared file lock.
    pub fn verify_chain(&self) -> Result<bool> {
        let Ok(file) = File::open(&self.path) else {
            return Ok(true);
        };
        file.lock_shared()?;
        let result = match read_events_from_file(&file) {
            Ok(events) => verify_events(&events),
            Err(ExecutionLedgerError::InvalidChain(_)) => Ok(false),
            Err(error) => Err(error),
        };
        let _ = FileExt::unlock(&file);
        result
    }

    /// Loads all well-formed events in file order.
    pub fn load(&self) -> Result<Vec<ExecutionEvent>> {
        let Ok(file) = File::open(&self.path) else {
            return Ok(Vec::new());
        };
        file.lock_shared()?;
        let result = read_events_from_file(&file);
        let _ = FileExt::unlock(&file);
        result
    }

    /// Loads one snapshot only when its record digests, chain, and lifecycle verify.
    pub fn load_verified(&self) -> Result<Vec<ExecutionEvent>> {
        let Ok(file) = File::open(&self.path) else {
            return Ok(Vec::new());
        };
        file.lock_shared()?;
        let result = read_events_from_file(&file).and_then(|events| {
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

    /// Returns all events associated with `task_id`.
    #[must_use]
    pub fn by_task(&self, task_id: &str) -> Vec<ExecutionEvent> {
        self.load_verified()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.task_id() == task_id)
            .collect()
    }

    /// Returns the last assigned sequence number, or zero for an empty/missing file.
    #[must_use]
    pub fn last_sequence(&self) -> u64 {
        self.load()
            .ok()
            .and_then(|events| events.last().map(ExecutionEvent::sequence_number))
            .unwrap_or(0)
    }
}

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
    let mut reader = BufReader::new(file.try_clone()?);
    reader.seek(SeekFrom::Start(0))?;
    let mut events = Vec::new();
    for (line_number, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            return Err(ExecutionLedgerError::InvalidRecord(format!(
                "empty line at index {line_number}"
            )));
        }
        let event = parse_canonical_event(&line)?;
        events.push(event);
    }
    Ok(events)
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
