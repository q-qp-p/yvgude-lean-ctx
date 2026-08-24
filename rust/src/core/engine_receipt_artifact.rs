//! Strict local resolution for context-bound Engine receipt artifacts.
//!
//! This module proves content identity and canonical cross-artifact linkage. It
//! deliberately does not establish signer trust, revocation, freshness, or
//! one-time admission consumption; those remain runtime-gate responsibilities.

use std::path::{Path, PathBuf};

use lean_ctx_protocol::{
    EngineInvocationV1, EngineObservationV1, EnginePolicyDecisionV1, InvocationContextBindingV1,
    InvocationSourceRoleV1, ProtocolReference, Sha256Digest,
};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::core::{canonical, engine_artifact};

const RECEIPT_DIRECTORY: &str = "engine-interface/v2/receipts";
const CONTEXT_BINDING_DIRECTORY: &str = "engine-interface/v1/context-bindings";
const MAX_ENGINE_RECEIPT_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_BINDING_BYTES: usize = 1024 * 1024;

/// A receipt envelope that commits a pre-link observation to its signed
/// invocation-context sidecar without creating a self-referential digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineReceiptArtifactV2 {
    #[serde(deserialize_with = "deserialize_v2_schema_version")]
    pub(crate) schema_version: u32,
    pub(crate) context_binding_digest: Sha256Digest,
    pub(crate) invocation: EngineInvocationV1,
    pub(crate) observation: EngineObservationV1,
}

impl EngineReceiptArtifactV2 {
    pub(crate) const SCHEMA_VERSION: u32 = 2;

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        Ok(canonical::canonical_serialize(self))
    }

    pub(crate) fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        let artifact: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("decode EngineReceiptArtifactV2: {error}"))?;
        let canonical = artifact.canonical_bytes()?;
        if canonical != bytes {
            return Err("EngineReceiptArtifactV2 JSON is not canonical".to_owned());
        }
        Ok(artifact)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err("EngineReceiptArtifactV2 schema_version must be 2".to_owned());
        }
        self.observation
            .validate_for(&self.invocation)
            .map_err(|error| error.to_string())?;
        if self.observation.receipt_link.is_some() {
            return Err(
                "EngineReceiptArtifactV2 observation must be stored before receipt linkage"
                    .to_owned(),
            );
        }
        Ok(())
    }
}

/// A content-verified receipt and its canonical context sidecar.
///
/// The binding signature is structurally validated but is not trusted here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedEngineReceiptArtifactV2 {
    pub(crate) receipt_digest: Sha256Digest,
    pub(crate) artifact: EngineReceiptArtifactV2,
    pub(crate) context_binding: InvocationContextBindingV1,
}

/// Resolve fixed-layout Engine artifacts beneath one rooted directory.
pub(crate) struct EngineReceiptArtifactResolverV2 {
    root: PathBuf,
}

impl EngineReceiptArtifactResolverV2 {
    pub(crate) fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub(crate) fn resolve(
        &self,
        receipt_ref: &ProtocolReference,
    ) -> Result<ResolvedEngineReceiptArtifactV2, String> {
        let receipt_digest = receipt_digest(receipt_ref)?;
        let receipt_bytes = engine_artifact::read_bounded_content(
            &self.root,
            RECEIPT_DIRECTORY,
            receipt_digest.hex(),
            "json",
            MAX_ENGINE_RECEIPT_BYTES,
        )?;
        require_digest(&receipt_bytes, &receipt_digest, "Engine receipt")?;
        let artifact = EngineReceiptArtifactV2::from_canonical_bytes(&receipt_bytes)?;

        let binding_digest = artifact.context_binding_digest.clone();
        let binding_bytes = engine_artifact::read_bounded_content(
            &self.root,
            CONTEXT_BINDING_DIRECTORY,
            binding_digest.hex(),
            "json",
            MAX_CONTEXT_BINDING_BYTES,
        )?;
        require_digest(
            &binding_bytes,
            &binding_digest,
            "invocation context binding",
        )?;
        let context_binding = InvocationContextBindingV1::from_canonical_bytes(&binding_bytes)
            .map_err(|error| format!("decode invocation context binding: {error}"))?;
        validate_context_binding(&context_binding, &artifact.invocation)?;

        Ok(ResolvedEngineReceiptArtifactV2 {
            receipt_digest,
            artifact,
            context_binding,
        })
    }
}

fn deserialize_v2_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value != EngineReceiptArtifactV2::SCHEMA_VERSION {
        return Err(serde::de::Error::custom(
            "EngineReceiptArtifactV2 schema_version must be 2",
        ));
    }
    Ok(value)
}

fn receipt_digest(receipt_ref: &ProtocolReference) -> Result<Sha256Digest, String> {
    let value = receipt_ref.as_str();
    let digest = value
        .strip_prefix("receipt:")
        .ok_or_else(|| "Engine receipt reference must use receipt:<sha256-digest>".to_owned())?;
    let digest = Sha256Digest::new(digest.to_owned()).map_err(|error| error.to_string())?;
    if value != format!("receipt:{}", digest.as_str()) {
        return Err(
            "Engine receipt reference must exactly equal receipt:<sha256-digest>".to_owned(),
        );
    }
    Ok(digest)
}

fn require_digest(bytes: &[u8], expected: &Sha256Digest, label: &str) -> Result<(), String> {
    if &digest_bytes(bytes)? != expected {
        return Err(format!("{label} content differs from addressed digest"));
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> Result<Sha256Digest, String> {
    Sha256Digest::new(format!(
        "sha256:{}",
        crate::core::agent_identity::hex_encode(&Sha256::digest(bytes))
    ))
    .map_err(|error| error.to_string())
}

fn validate_context_binding(
    binding: &InvocationContextBindingV1,
    invocation: &EngineInvocationV1,
) -> Result<(), String> {
    if binding.invocation_id != invocation.invocation_id {
        return Err("context binding invocation_id does not match Engine invocation".to_owned());
    }
    let invocation_digest = digest_bytes(&canonical::canonical_serialize(invocation))?;
    if binding.invocation_ref != invocation_digest {
        return Err("context binding invocation_ref does not match Engine invocation".to_owned());
    }
    if binding.policy_ref != invocation.policy_admission.policy_ref
        || binding.decision != invocation.policy_admission.decision
        || binding.decision != EnginePolicyDecisionV1::Admitted
    {
        return Err("context binding policy admission does not match Engine invocation".to_owned());
    }
    if binding.source_bindings.len() != invocation.source_refs.len()
        || invocation.source_refs.iter().any(|source_ref| {
            !binding
                .source_bindings
                .iter()
                .any(|source_binding| &source_binding.source_ref == source_ref)
        })
    {
        return Err("context binding sources do not exactly match Engine invocation".to_owned());
    }
    let input = binding
        .source_bindings
        .iter()
        .find(|source| source.role == InvocationSourceRoleV1::Input)
        .ok_or_else(|| "context binding has no input source".to_owned())?;
    if input.source_ref != invocation.input_ref || input.digest != invocation.input_digest {
        return Err("context binding input does not match Engine invocation".to_owned());
    }
    if binding.capability_bindings.len() != 1
        || binding.capability_bindings[0].capability_id != invocation.operation.capability_id
        || binding.capability_bindings[0].capability_version
            != invocation.operation.capability_version
    {
        return Err("context binding capability does not match Engine invocation".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use lean_ctx_protocol::{CapabilityId, SemanticVersion};
    use lean_ctx_protocol::{EngineInvocationIdV1, EngineReceiptLinkV1, ReceiptId};

    const INVOCATION_JSON: &str = r#"{
        "schema_version":1,
        "invocation_id":"invocation-1",
        "engine":{"engine_id":"lean-ctx-local","engine_version":"1.0.0"},
        "operation":{"capability_id":"capability:engine","capability_version":"1.0.0"},
        "input_ref":"source:input",
        "input_digest":"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "source_refs":["source:input"],
        "policy_admission":{"policy_ref":"policy:invocation-admission","decision":"admitted"}
    }"#;

    const OBSERVATION_JSON: &str = r#"{
        "schema_version":1,
        "invocation_id":"invocation-1",
        "status":"succeeded",
        "output_ref":"output:result",
        "output_digest":"sha256:3333333333333333333333333333333333333333333333333333333333333333",
        "source_lineage":["source:input"],
        "measurements":[]
    }"#;

    fn fixture() -> (EngineReceiptArtifactV2, InvocationContextBindingV1) {
        let invocation: EngineInvocationV1 =
            serde_json::from_str(INVOCATION_JSON).expect("invocation fixture");
        let observation: EngineObservationV1 =
            serde_json::from_str(OBSERVATION_JSON).expect("observation fixture");
        let binding_bytes =
            include_bytes!("../../../docs/contracts/invocation-context-binding/v1/valid.json");
        let binding_bytes = binding_bytes.strip_suffix(b"\n").unwrap_or(binding_bytes);
        let mut binding = InvocationContextBindingV1::from_canonical_bytes(binding_bytes)
            .expect("binding fixture");
        binding.invocation_ref =
            digest_bytes(&canonical::canonical_serialize(&invocation)).expect("invocation digest");
        let binding_digest = binding.digest().expect("binding digest");
        (
            EngineReceiptArtifactV2 {
                schema_version: EngineReceiptArtifactV2::SCHEMA_VERSION,
                context_binding_digest: binding_digest,
                invocation,
                observation,
            },
            binding,
        )
    }

    #[cfg(unix)]
    fn write_content(root: &Path, directory: &str, digest: &Sha256Digest, bytes: &[u8]) {
        let directory = root.join(directory);
        std::fs::create_dir_all(&directory).expect("artifact directory");
        std::fs::write(directory.join(format!("{}.json", digest.hex())), bytes)
            .expect("artifact bytes");
    }

    #[cfg(unix)]
    fn resolved_fixture() -> (
        tempfile::TempDir,
        ProtocolReference,
        EngineReceiptArtifactV2,
        InvocationContextBindingV1,
    ) {
        let root = tempfile::tempdir().expect("temporary artifact root");
        let (artifact, binding) = fixture();
        let binding_bytes = binding.canonical_bytes().expect("canonical binding");
        write_content(
            root.path(),
            CONTEXT_BINDING_DIRECTORY,
            &artifact.context_binding_digest,
            &binding_bytes,
        );
        let receipt_bytes = artifact.canonical_bytes().expect("canonical receipt");
        let receipt_digest = digest_bytes(&receipt_bytes).expect("receipt digest");
        write_content(
            root.path(),
            RECEIPT_DIRECTORY,
            &receipt_digest,
            &receipt_bytes,
        );
        let receipt_ref = ProtocolReference::new(format!("receipt:{}", receipt_digest.as_str()))
            .expect("receipt reference");
        (root, receipt_ref, artifact, binding)
    }

    #[cfg(unix)]
    #[test]
    fn resolves_exact_canonical_receipt_and_binding() {
        let (root, receipt_ref, artifact, binding) = resolved_fixture();
        let resolved = EngineReceiptArtifactResolverV2::with_root(root.path())
            .resolve(&receipt_ref)
            .expect("resolve receipt");

        assert_eq!(resolved.artifact, artifact);
        assert_eq!(resolved.context_binding, binding);
        assert_eq!(
            receipt_ref.as_str(),
            format!("receipt:{}", resolved.receipt_digest.as_str())
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_tampering_noncanonical_json_and_unknown_fields() {
        let (root, receipt_ref, _, _) = resolved_fixture();
        let digest = receipt_digest(&receipt_ref).expect("receipt digest");
        let path = root
            .path()
            .join(RECEIPT_DIRECTORY)
            .join(format!("{}.json", digest.hex()));
        let mut bytes = std::fs::read(&path).expect("receipt bytes");
        bytes[0] = b'[';
        std::fs::write(&path, bytes).expect("tamper receipt");
        assert!(
            EngineReceiptArtifactResolverV2::with_root(root.path())
                .resolve(&receipt_ref)
                .unwrap_err()
                .contains("content differs")
        );

        let root = tempfile::tempdir().expect("temporary artifact root");
        let (artifact, binding) = fixture();
        let binding_bytes = binding.canonical_bytes().expect("binding bytes");
        write_content(
            root.path(),
            CONTEXT_BINDING_DIRECTORY,
            &artifact.context_binding_digest,
            &binding_bytes,
        );
        for mut receipt_bytes in [artifact.canonical_bytes().expect("receipt bytes"), {
            let mut value = serde_json::to_value(&artifact).expect("receipt value");
            value
                .as_object_mut()
                .expect("receipt object")
                .insert("unexpected".to_owned(), serde_json::json!(true));
            canonical::canonical_serialize(&value)
        }] {
            if !receipt_bytes
                .windows(12)
                .any(|part| part == b"\"unexpected\"")
            {
                receipt_bytes.push(b'\n');
            }
            let digest = digest_bytes(&receipt_bytes).expect("receipt digest");
            write_content(root.path(), RECEIPT_DIRECTORY, &digest, &receipt_bytes);
            let receipt_ref = ProtocolReference::new(format!("receipt:{}", digest.as_str()))
                .expect("receipt reference");
            assert!(
                EngineReceiptArtifactResolverV2::with_root(root.path())
                    .resolve(&receipt_ref)
                    .is_err()
            );
        }
    }

    #[test]
    fn rejects_duplicate_fields_linked_observations_and_cross_links() {
        let (mut artifact, _) = fixture();
        let canonical = artifact.canonical_bytes().expect("canonical receipt");
        let mut duplicate = b"{\"schema_version\":2,".to_vec();
        duplicate.extend_from_slice(&canonical[1..]);
        assert!(EngineReceiptArtifactV2::from_canonical_bytes(&duplicate).is_err());

        artifact.observation.receipt_link = Some(EngineReceiptLinkV1 {
            schema_version: 1,
            receipt_id: ReceiptId::new("engine-receipt-linked").expect("receipt id"),
            receipt_ref: ProtocolReference::new(format!("receipt:sha256:{}", "4".repeat(64)))
                .expect("receipt ref"),
            receipt_digest: Sha256Digest::new(format!("sha256:{}", "4".repeat(64)))
                .expect("receipt digest"),
            invocation_id: artifact.invocation.invocation_id.clone(),
        });
        let linked = canonical::canonical_serialize(&artifact);
        assert!(
            EngineReceiptArtifactV2::from_canonical_bytes(&linked)
                .unwrap_err()
                .contains("before receipt linkage")
        );

        let (artifact, mut mismatched) = fixture();
        mismatched.invocation_id =
            EngineInvocationIdV1::new("invocation-other").expect("invocation id");
        assert!(
            validate_context_binding(&mismatched, &artifact.invocation)
                .unwrap_err()
                .contains("invocation_id")
        );

        let (_, mut mismatched) = fixture();
        mismatched.policy_ref = ProtocolReference::new("policy:other").expect("policy reference");
        assert!(
            validate_context_binding(&mismatched, &artifact.invocation)
                .unwrap_err()
                .contains("policy admission")
        );

        let (_, mut mismatched) = fixture();
        mismatched.source_bindings[0].digest =
            Sha256Digest::new(format!("sha256:{}", "7".repeat(64))).expect("source digest");
        assert!(
            validate_context_binding(&mismatched, &artifact.invocation)
                .unwrap_err()
                .contains("input")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_wrong_sidecar_identity_and_cross_artifact_mix() {
        let root = tempfile::tempdir().expect("temporary artifact root");
        let (mut artifact, binding) = fixture();
        let binding_bytes = binding.canonical_bytes().expect("binding bytes");
        artifact.context_binding_digest =
            Sha256Digest::new(format!("sha256:{}", "5".repeat(64))).expect("wrong digest");
        write_content(
            root.path(),
            CONTEXT_BINDING_DIRECTORY,
            &artifact.context_binding_digest,
            &binding_bytes,
        );
        let receipt_bytes = artifact.canonical_bytes().expect("receipt bytes");
        let digest = digest_bytes(&receipt_bytes).expect("receipt digest");
        write_content(root.path(), RECEIPT_DIRECTORY, &digest, &receipt_bytes);
        let receipt_ref =
            ProtocolReference::new(format!("receipt:{}", digest.as_str())).expect("receipt ref");
        assert!(
            EngineReceiptArtifactResolverV2::with_root(root.path())
                .resolve(&receipt_ref)
                .unwrap_err()
                .contains("context binding content differs")
        );

        let root = tempfile::tempdir().expect("temporary artifact root");
        let (mut artifact, mut binding) = fixture();
        binding.capability_bindings[0].capability_id =
            CapabilityId::new("capability:other").expect("capability id");
        binding.capability_bindings[0].capability_version =
            SemanticVersion::new("9.0.0").expect("capability version");
        let binding_bytes = binding.canonical_bytes().expect("binding bytes");
        artifact.context_binding_digest = digest_bytes(&binding_bytes).expect("binding digest");
        write_content(
            root.path(),
            CONTEXT_BINDING_DIRECTORY,
            &artifact.context_binding_digest,
            &binding_bytes,
        );
        let receipt_bytes = artifact.canonical_bytes().expect("receipt bytes");
        let digest = digest_bytes(&receipt_bytes).expect("receipt digest");
        write_content(root.path(), RECEIPT_DIRECTORY, &digest, &receipt_bytes);
        let receipt_ref =
            ProtocolReference::new(format!("receipt:{}", digest.as_str())).expect("receipt ref");
        assert!(
            EngineReceiptArtifactResolverV2::with_root(root.path())
                .resolve(&receipt_ref)
                .unwrap_err()
                .contains("capability")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_oversized_symlinked_and_wrong_namespace_artifacts() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary artifact root");
        let bytes = vec![b'x'; MAX_ENGINE_RECEIPT_BYTES + 1];
        let digest = digest_bytes(&bytes).expect("oversized digest");
        write_content(root.path(), RECEIPT_DIRECTORY, &digest, &bytes);
        let receipt_ref =
            ProtocolReference::new(format!("receipt:{}", digest.as_str())).expect("receipt ref");
        assert_eq!(
            EngineReceiptArtifactResolverV2::with_root(root.path())
                .resolve(&receipt_ref)
                .unwrap_err(),
            "engine_artifact_size_limit_exceeded"
        );

        let root = tempfile::tempdir().expect("temporary artifact root");
        let outside = tempfile::tempdir().expect("outside directory");
        std::fs::create_dir(root.path().join("engine-interface")).expect("engine directory");
        symlink(
            outside.path(),
            root.path().join("engine-interface").join("v2"),
        )
        .expect("directory symlink");
        assert!(
            EngineReceiptArtifactResolverV2::with_root(root.path())
                .resolve(&receipt_ref)
                .is_err()
        );

        let root = tempfile::tempdir().expect("temporary artifact root");
        let receipt_directory = root.path().join(RECEIPT_DIRECTORY);
        std::fs::create_dir_all(&receipt_directory).expect("receipt directory");
        let fifo_path = receipt_directory.join(format!("{}.json", digest.hex()));
        let fifo_path = CString::new(fifo_path.as_os_str().as_bytes()).expect("FIFO path");
        // SAFETY: fifo_path is a valid NUL-terminated path in the temporary directory.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        assert_eq!(
            EngineReceiptArtifactResolverV2::with_root(root.path())
                .resolve(&receipt_ref)
                .unwrap_err(),
            "engine_artifact_leaf_untrusted"
        );

        let wrong =
            ProtocolReference::new(format!("id:{}", digest.as_str())).expect("wrong namespace ref");
        assert!(receipt_digest(&wrong).is_err());
    }
}
