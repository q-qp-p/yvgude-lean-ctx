//! Open Context & Token Lifecycle Architecture (OCLA) — the stable contract
//! boundary between lean-ctx-core (Apache-2.0 OSS) and lean-ctx-enterprise
//! (proprietary). All 14 OCLA traits, canonical types, token envelopes,
//! and agent message primitives live here.
//!
//! Dependency direction:
//!   lean-ctx-core depends on lean-ctx-ocla (OSS → OSS)
//!   lean-ctx-enterprise depends on lean-ctx-ocla (Proprietary → OSS)
//!   lean-ctx-ocla depends on NOTHING from lean-ctx-core or enterprise

pub mod manifest;
pub mod traits;
pub mod types;

pub use traits::*;
pub use types::*;
