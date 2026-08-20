//! Capability manifest and registry primitives for OCLA.
//!
//! This module is intentionally independent from the existing runtime service
//! registry in [`crate::core::ocla`]. It provides the v0 capability declaration
//! substrate that later orchestration layers can consume.

pub mod builtins;
pub mod manifest;
pub mod registry;

pub use manifest::{
    CapabilityManifest, CapabilityProperties, CapabilityType, ExecutionMode, IOContract, Permission,
};
pub use registry::{CapabilityRegistry, RegistryError, Result};

#[cfg(test)]
mod tests;
