//! Hash-based keyword identity with global name recovery.
//!
//! Keywords are stored as tagged-union values where the payload holds an FNV-1a
//! hash of the keyword name. The hash is deterministic across runs, threads, and
//! DSO boundaries. Equality is `u64 == u64` — no string comparison, no heap
//! dereference.
//!
//! The global name table (`KEYWORD_NAMES`) maps hashes back to names for display
//! and pattern matching. Every keyword ever created via `Value::keyword()` has
//! its name in the table.
//!
//! ## Plugin keyword routing
//!
//! Plugins using the stable ABI (`elle-plugin` crate) do not link against
//! `elle` and never access `KEYWORD_NAMES` directly. Instead, they call
//! `api().keyword("name")` which routes through the named-function ABI to
//! `make_keyword` in `plugin_api.rs`, which calls `Value::keyword()` in the
//! host — automatically using the host's keyword table.
//!
//! ## Collision handling
//!
//! Hash collisions panic immediately — different names that produce the same
//! 64-bit hash are a fatal, unrecoverable condition. The payload is a full
//! `u64` (the keyword tag occupies the separate tag word of the 16-byte
//! `Value`), so the hash uses all 64 bits. At realistic keyword set sizes
//! (≤ 10,000), the birthday-bound collision probability is below
//! 0.0000000003% (~1 in 3·10^11).

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Global keyword name table. Maps 64-bit FNV-1a hash to keyword name.
///
/// This is the authoritative table in the host binary. Stable-ABI plugins
/// never access it directly — they call through the named-function API
/// (`intern_keyword`, `keyword_name` in `plugin_api.rs`), which operates
/// on this table in the host process.
static KEYWORD_NAMES: LazyLock<RwLock<HashMap<u64, Box<str>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Full 64-bit FNV-1a hash of a keyword name.
///
/// No truncation: the keyword payload is a full `u64` in the 16-byte
/// `Value`, so the hash spans the entire 64-bit space for maximum
/// collision resistance.
///
/// `const fn` to enable precomputed keyword hash constants in the future.
/// Uses `while` loop (not `for`) because `for` desugars to
/// `IntoIterator::into_iter()` which is not const-compatible.
pub const fn keyword_hash(name: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000000000001b;
    let bytes = name.as_bytes();
    let mut hash = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

/// Register a keyword name and return its 64-bit hash.
///
/// Panics on hash collision (different name maps to same hash).
/// RwLock poisoning on collision panic is intentional — a collision
/// is fatal and the process should abort.
pub fn intern_keyword(name: &str) -> u64 {
    let hash = keyword_hash(name);
    {
        let table = KEYWORD_NAMES.read().unwrap();
        if let Some(existing) = table.get(&hash) {
            assert!(
                &**existing == name,
                "keyword hash collision: {:?} and {:?} both hash to {:#x}",
                existing,
                name,
                hash
            );
            return hash;
        }
    }
    let mut table = KEYWORD_NAMES.write().unwrap();
    if let Some(existing) = table.get(&hash) {
        assert!(
            &**existing == name,
            "keyword hash collision: {:?} and {:?} both hash to {:#x}",
            existing,
            name,
            hash
        );
    } else {
        table.insert(hash, name.into());
    }
    hash
}

/// Return the number of registered keywords in the global name table.
pub fn keyword_count() -> usize {
    KEYWORD_NAMES.read().unwrap().len()
}

/// Look up a keyword name by its 64-bit hash.
///
/// Returns None only if the hash was never registered — should not happen
/// for any keyword created through Value::keyword().
pub fn keyword_name(hash: u64) -> Option<String> {
    KEYWORD_NAMES
        .read()
        .unwrap()
        .get(&hash)
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests;
