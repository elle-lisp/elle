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
//! 47-bit hash are a fatal, unrecoverable condition. At realistic keyword
//! set sizes (≤ 10,000), the probability of collision is < 0.00004%.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Global keyword name table. Maps 47-bit FNV-1a hash to keyword name.
///
/// This is the authoritative table in the host binary. Stable-ABI plugins
/// never access it directly — they call through the named-function API
/// (`intern_keyword`, `keyword_name` in `plugin_api.rs`), which operates
/// on this table in the host process.
static KEYWORD_NAMES: LazyLock<RwLock<HashMap<u64, Box<str>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// FNV-1a 64-bit hash of a keyword name, truncated to 47 bits.
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
    hash & ((1u64 << 47) - 1)
}

/// Register a keyword name and return its 47-bit hash.
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

/// Look up a keyword name by its 47-bit hash.
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
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(keyword_hash("foo"), keyword_hash("foo"));
        assert_eq!(keyword_hash("error"), keyword_hash("error"));
    }

    #[test]
    fn hash_fits_in_47_bits() {
        let h = keyword_hash("some-long-keyword-name-that-exercises-the-hash");
        assert_eq!(h & !((1u64 << 47) - 1), 0, "hash must fit in 47 bits");
    }

    #[test]
    fn hash_is_const() {
        const H: u64 = keyword_hash("test");
        assert_eq!(H & !((1u64 << 47) - 1), 0, "const hash must fit in 47 bits");
    }

    #[test]
    fn known_keywords_no_collision() {
        let keywords = [
            "error",
            "yield",
            "type-error",
            "arity-error",
            "exec-error",
            "encoding-error",
            "sparql-error",
            "oxigraph-error",
            "iri",
            "bnode",
            "literal",
            "lang",
            "datatype",
            "turtle",
            "ntriples",
            "nquads",
            "rdfxml",
            "subject",
            "predicate",
            "object",
            "s",
            "p",
            "o",
            "g",
            "exit",
            "stdout",
            "stderr",
            "cwd",
            "env",
            "stdin",
            "null",
            "pipe",
            "ok",
            "err",
        ];
        let mut seen = std::collections::HashMap::new();
        for kw in &keywords {
            let h = keyword_hash(kw);
            if let Some(prev) = seen.insert(h, kw) {
                panic!("collision: {:?} and {:?} both hash to {:#x}", prev, kw, h);
            }
        }
    }

    #[test]
    fn intern_and_lookup() {
        let h = intern_keyword("test-intern-lookup");
        assert_eq!(keyword_name(h).as_deref(), Some("test-intern-lookup"));
    }

    #[test]
    fn intern_idempotent() {
        let h1 = intern_keyword("test-idempotent");
        let h2 = intern_keyword("test-idempotent");
        assert_eq!(h1, h2);
    }
}
