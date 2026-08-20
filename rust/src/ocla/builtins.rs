//! Manifests for LeanCTX's built-in OCLA capabilities.

use semver::Version;

use super::{
    CapabilityManifest, CapabilityProperties, CapabilityType, ExecutionMode, IOContract, Permission,
};

/// Return the manifest for LeanCTX's deterministic structural compression provider.
#[must_use]
pub fn structural_compression_manifest() -> CapabilityManifest {
    CapabilityManifest {
        id: "leanctx.compression.structural".to_owned(),
        version: Version::new(0, 1, 0),
        capability_type: CapabilityType::Compression,
        execution_mode: ExecutionMode::InProcess,
        input_contract: IOContract {
            content_type: "text/plain".to_owned(),
            max_size_bytes: None,
            schema: None,
        },
        output_contract: IOContract {
            content_type: "text/plain".to_owned(),
            max_size_bytes: None,
            schema: None,
        },
        properties: CapabilityProperties {
            lossy: true,
            recoverable: false,
            cache_safe: true,
            deterministic: true,
            max_latency_ms: None,
        },
        permissions: vec![Permission::ReadFileSystem, Permission::WriteFileSystem],
    }
}
