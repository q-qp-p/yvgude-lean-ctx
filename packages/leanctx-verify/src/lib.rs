//! Standalone, provider-free LeanCTX evidence verification library.
//!
//! This crate intentionally has no dependency on the Engine. Engine tests may
//! depend on this crate to prove producer/verifier independence.

mod receipt;
pub mod v2;
pub mod verify;
