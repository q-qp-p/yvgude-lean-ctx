use semver::Version;

use super::{
    CapabilityManifest, CapabilityProperties, CapabilityRegistry, CapabilityType, ExecutionMode,
    IOContract, Permission, RegistryError, builtins,
};

fn test_manifest() -> CapabilityManifest {
    CapabilityManifest {
        id: "leanctx.retrieval.test".to_owned(),
        version: Version::new(0, 1, 0),
        capability_type: CapabilityType::Retrieval,
        execution_mode: ExecutionMode::InProcess,
        input_contract: IOContract {
            content_type: "application/json".to_owned(),
            max_size_bytes: Some(1_024),
            schema: Some("https://leanctx.dev/schemas/retrieval-input.json".to_owned()),
        },
        output_contract: IOContract {
            content_type: "application/json".to_owned(),
            max_size_bytes: Some(4_096),
            schema: Some("https://leanctx.dev/schemas/retrieval-output.json".to_owned()),
        },
        properties: CapabilityProperties {
            lossy: false,
            recoverable: true,
            cache_safe: true,
            deterministic: true,
            max_latency_ms: Some(100),
        },
        permissions: vec![Permission::ReadFileSystem],
    }
}

#[test]
fn manifest_round_trips_through_json() {
    let manifest = test_manifest();

    let serialized = serde_json::to_string(&manifest).expect("manifest serializes");
    let restored: CapabilityManifest =
        serde_json::from_str(&serialized).expect("manifest deserializes");

    assert_eq!(restored, manifest);
    assert!(serialized.contains("\"version\":\"0.1.0\""));
}

#[test]
fn registry_registers_and_looks_up_capabilities() {
    let manifest = test_manifest();
    let mut registry = CapabilityRegistry::new();

    registry
        .register(manifest.clone())
        .expect("manifest is valid");

    assert_eq!(registry.get(&manifest.id), Some(&manifest));
    assert_eq!(
        registry.list_by_type(CapabilityType::Retrieval),
        vec![&manifest]
    );
    assert!(
        registry
            .list_by_type(CapabilityType::Compression)
            .is_empty()
    );
    assert_eq!(
        registry.register(manifest).unwrap_err(),
        RegistryError::DuplicateCapability("leanctx.retrieval.test".to_owned())
    );
}

#[test]
fn validation_rejects_invalid_manifests() {
    let mut manifest = test_manifest();
    manifest.id = " ".to_owned();

    assert_eq!(
        CapabilityRegistry::validate(&manifest).unwrap_err(),
        RegistryError::EmptyField("id")
    );

    manifest.id = "leanctx.retrieval.test".to_owned();
    manifest.execution_mode = ExecutionMode::Remote {
        endpoint: " ".to_owned(),
    };
    assert_eq!(
        CapabilityRegistry::validate(&manifest).unwrap_err(),
        RegistryError::EmptyRemoteEndpoint
    );
}

#[test]
fn builtin_structural_compression_manifest_loads() {
    let manifest = builtins::structural_compression_manifest();
    let mut registry = CapabilityRegistry::new();

    registry
        .register(manifest.clone())
        .expect("builtin manifest is valid");

    assert_eq!(manifest.id, "leanctx.compression.structural");
    assert_eq!(manifest.capability_type, CapabilityType::Compression);
    assert_eq!(manifest.execution_mode, ExecutionMode::InProcess);
    assert!(manifest.properties.lossy);
    assert!(manifest.properties.deterministic);
    assert!(manifest.permissions.contains(&Permission::ReadFileSystem));
    assert_eq!(registry.get(&manifest.id), Some(&manifest));
}
