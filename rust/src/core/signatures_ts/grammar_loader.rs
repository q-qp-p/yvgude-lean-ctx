//! Runtime loader for grammar-addon dylibs (#690, Phase 1b).
//!
//! Addon-backed grammar loading was removed with the addon subsystem; callers
//! fall through to statically linked grammars and regex-signature extractors.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tree_sitter::Language;

/// Cached entry point for `queries::get_language`'s addon fallback.
pub(super) fn get_addon_language(ext: &str) -> Option<Language> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Language>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(ext)
    {
        return hit.clone();
    }

    let result = None::<Language>;
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(ext.to_string())
        .or_insert(result)
        .clone()
}
