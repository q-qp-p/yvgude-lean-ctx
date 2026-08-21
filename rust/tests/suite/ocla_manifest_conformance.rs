//! Repository conformance checks for OCLA capability manifests.

use lean_ctx_ocla::manifest::validate_manifest;
use lean_ctx_protocol::CapabilityManifestV1;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
enum MockRegistryError {
    DuplicateCapabilityId(String),
}

#[derive(Default)]
struct MockRegistry {
    capability_ids: BTreeSet<String>,
}

impl MockRegistry {
    fn register(&mut self, manifest: &CapabilityManifestV1) -> Result<(), MockRegistryError> {
        let capability_id = manifest.capability_id.as_str().to_owned();
        if self.capability_ids.insert(capability_id.clone()) {
            Ok(())
        } else {
            Err(MockRegistryError::DuplicateCapabilityId(capability_id))
        }
    }
}

fn manifest_paths(directory: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
    {
        let path = entry
            .unwrap_or_else(|error| panic!("read entry in {}: {error}", directory.display()))
            .path();
        if path.is_dir() {
            paths.extend(manifest_paths(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

fn load_manifest(path: &Path) -> CapabilityManifestV1 {
    let json =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&json)
        .unwrap_or_else(|error| panic!("deserialize {}: {error}", path.display()))
}

fn manifest_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/contracts/ocla/capability-manifests")
}

#[test]
fn repository_manifests_deserialize_and_validate() {
    let paths = manifest_paths(&manifest_directory());
    assert!(
        !paths.is_empty(),
        "repository must publish capability manifests"
    );

    let mut registry = MockRegistry::default();
    for path in paths {
        let manifest = load_manifest(&path);
        assert_eq!(manifest.schema_version, 1, "{}", path.display());
        assert!(
            !manifest.capability_id.as_str().trim().is_empty(),
            "{}",
            path.display()
        );
        assert!(!manifest.surfaces.is_empty(), "{}", path.display());
        validate_manifest(&manifest)
            .unwrap_or_else(|error| panic!("validate {}: {error}", path.display()));
        registry
            .register(&manifest)
            .unwrap_or_else(|error| panic!("register {}: {error:?}", path.display()));
    }
}

#[test]
fn mock_registry_rejects_duplicate_capability_ids() {
    let path = manifest_paths(&manifest_directory())
        .into_iter()
        .next()
        .expect("repository must publish a capability manifest");
    let manifest = load_manifest(&path);
    let mut registry = MockRegistry::default();

    registry
        .register(&manifest)
        .expect("first registration succeeds");
    assert_eq!(
        registry.register(&manifest),
        Err(MockRegistryError::DuplicateCapabilityId(
            manifest.capability_id.as_str().to_owned()
        ))
    );
}
