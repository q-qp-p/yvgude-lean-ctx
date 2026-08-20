//! Versioned declarations for independently orchestrated capabilities.

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A versioned declaration of a single OCLA capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    /// Globally stable capability identifier, for example
    /// `leanctx.compression.structural`.
    pub id: String,
    /// Semantic version of the capability implementation and contract.
    #[serde(with = "version_serde")]
    pub version: Version,
    /// Functional category used for capability discovery.
    pub capability_type: CapabilityType,
    /// Where the capability executes.
    pub execution_mode: ExecutionMode,
    /// Input content contract.
    pub input_contract: IOContract,
    /// Output content contract.
    pub output_contract: IOContract,
    /// Behavioural and performance claims.
    pub properties: CapabilityProperties,
    /// Privileges required by the capability.
    pub permissions: Vec<Permission>,
}

/// Functional category of a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityType {
    Compression,
    Retrieval,
    Caching,
    Selection,
    Recovery,
    Measurement,
    Routing,
}

/// Execution environment for a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    InProcess,
    LocalBinary,
    Remote { endpoint: String },
}

/// Input or output shape accepted by a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IOContract {
    /// MIME type of the content, such as `text/plain` or `application/json`.
    pub content_type: String,
    /// Optional maximum size accepted or produced by the capability.
    pub max_size_bytes: Option<u64>,
    /// Optional JSON Schema reference that further constrains the content.
    pub schema: Option<String>,
}

/// Behavioural and latency characteristics declared by a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityProperties {
    pub lossy: bool,
    pub recoverable: bool,
    pub cache_safe: bool,
    pub deterministic: bool,
    pub max_latency_ms: Option<u64>,
}

/// Privileges a capability needs to perform its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    ReadFileSystem,
    WriteFileSystem,
    NetworkAccess,
    ModelInference,
    ShellExecution,
}

/// Serialize semantic versions as their canonical string representation without
/// requiring the optional `semver` serde feature in the workspace dependency.
mod version_serde {
    use super::*;

    pub(super) fn serialize<S>(version: &Version, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&version.to_string())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Version, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Version::parse(&value).map_err(serde::de::Error::custom)
    }
}
