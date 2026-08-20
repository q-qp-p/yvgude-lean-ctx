//! In-memory registry for validated OCLA capability manifests.

use std::collections::HashMap;

use thiserror::Error;

use super::{CapabilityManifest, CapabilityType, ExecutionMode, IOContract};

/// Errors returned while validating or registering a capability manifest.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("capability manifest field `{0}` must not be empty")]
    EmptyField(&'static str),
    #[error("capability manifest id `{0}` is invalid")]
    InvalidId(String),
    #[error("remote capability endpoint must not be empty")]
    EmptyRemoteEndpoint,
    #[error("capability `{0}` is already registered")]
    DuplicateCapability(String),
}

/// Result type for registry operations.
pub type Result<T> = std::result::Result<T, RegistryError>;

/// Stores validated capability manifests by their stable identifier.
#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    capabilities: HashMap<String, CapabilityManifest>,
}

impl CapabilityRegistry {
    /// Create an empty capability registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate and register a manifest, rejecting duplicate capability IDs.
    pub fn register(&mut self, manifest: CapabilityManifest) -> Result<()> {
        Self::validate(&manifest)?;

        if self.capabilities.contains_key(&manifest.id) {
            return Err(RegistryError::DuplicateCapability(manifest.id));
        }

        self.capabilities.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    /// Look up a manifest by its stable capability identifier.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&CapabilityManifest> {
        self.capabilities.get(id)
    }

    /// List registered manifests belonging to a capability category.
    #[must_use]
    pub fn list_by_type(&self, cap_type: CapabilityType) -> Vec<&CapabilityManifest> {
        let mut manifests = self
            .capabilities
            .values()
            .filter(|manifest| manifest.capability_type == cap_type)
            .collect::<Vec<_>>();
        manifests.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        manifests
    }

    /// Validate the v0 manifest shape before it is admitted to a registry.
    pub fn validate(manifest: &CapabilityManifest) -> Result<()> {
        validate_id(&manifest.id)?;
        validate_contract(&manifest.input_contract, "input_contract.content_type")?;
        validate_contract(&manifest.output_contract, "output_contract.content_type")?;

        if let ExecutionMode::Remote { endpoint } = &manifest.execution_mode {
            if endpoint.trim().is_empty() {
                return Err(RegistryError::EmptyRemoteEndpoint);
            }
        }

        Ok(())
    }
}

fn validate_id(id: &str) -> Result<()> {
    if id.trim().is_empty() {
        return Err(RegistryError::EmptyField("id"));
    }

    let valid = id.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    });

    if valid {
        Ok(())
    } else {
        Err(RegistryError::InvalidId(id.to_owned()))
    }
}

fn validate_contract(contract: &IOContract, content_type_field: &'static str) -> Result<()> {
    if contract.content_type.trim().is_empty() {
        return Err(RegistryError::EmptyField(content_type_field));
    }

    if contract
        .schema
        .as_deref()
        .is_some_and(|schema| schema.trim().is_empty())
    {
        return Err(RegistryError::EmptyField("contract.schema"));
    }

    Ok(())
}
