//! Hash-based keyword identity with global name recovery.
//!
//! Keywords are stored as NaN-boxed values where bits 0-46 hold a 47-bit FNV-1a
//! hash of the keyword name. The hash is deterministic across runs, threads, and
//! DSO boundaries. Equality is `u64 == u64` — no string comparison, no heap
//! dereference.
//!
//! The global name table (`KEYWORD_NAMES`) maps hashes back to names for display
//! and pattern matching. Every keyword ever created via `Value::keyword()` has
//! its name in the table.
//!
//! ## DSO boundary handling
//!
//! Each cdylib plugin compiled against the `elle` crate gets its own copy of
//! `KEYWORD_NAMES` (Rust statics are NOT shared across DSO boundaries). Plugins
//! must call `PluginContext::init_keywords()` at the start of `elle_plugin_init`
//! to install function pointer overrides that route `intern_keyword` and
//! `keyword_name` to the host's table. See `set_keyword_fns`.
//!
//! ## Collision handling
//!
//! Hash collisions panic immediately — different names that produce the same
//! 47-bit hash are a fatal, unrecoverable condition. At realistic keyword
//! set sizes (≤ 10,000), the probability of collision is < 0.00004%.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, RwLock};

/// Global keyword name table. Maps 47-bit FNV-1a hash to keyword name.
///
/// In the host binary, this is the authoritative table. In cdylib plugins,
/// each DSO gets its own copy of this static (Rust statics are NOT shared
/// across DSO boundaries — symbol names include a hash of the crate instance).
/// Plugins must call `set_keyword_fns` during init to route all keyword
/// operations to the host's copy of this table.
static KEYWORD_NAMES: LazyLock<RwLock<HashMap<u64, Box<str>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Override function pointers (as raw addresses), installed in cdylib plugins by `set_keyword_fns`.
///
/// When non-zero, `intern_keyword` and `keyword_name` delegate to these functions
/// instead of using the local `KEYWORD_NAMES` table. This enables plugins to
/// route keyword operations to the host's table despite each DSO having its
/// own copy of the `elle` crate's statics.
///
/// Stored as `usize` (raw address) to avoid niche-optimization issues with
/// function pointer types in `OnceLock`.
static INTERN_FN_PTR: AtomicUsize = AtomicUsize::new(0);
static LOOKUP_FN_PTR: AtomicUsize = AtomicUsize::new(0);

/// Install override functions for `intern_keyword` and `keyword_name`.
///
/// Called once by cdylib plugins during initialization (via `PluginContext::init_keywords`).
/// The host passes its own `intern_keyword` and `keyword_name` functions as pointers.
/// Subsequent calls to `intern_keyword`/`keyword_name` in this DSO will delegate to
/// the host's implementations, routing all accesses to the host's single table.
///
/// Must be called before any keyword is interned in this DSO.
pub fn set_keyword_fns(intern: fn(&str) -> u64, lookup: fn(u64) -> Option<String>) {
    INTERN_FN_PTR.store(intern as usize, Ordering::Release);
    LOOKUP_FN_PTR.store(lookup as usize, Ordering::Release);
}

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
/// In the host binary, registers in the local `KEYWORD_NAMES` table.
/// In cdylib plugins (after `set_keyword_fns` is called), delegates to the
/// host's `intern_keyword` function to register in the host's table.
///
/// Panics on hash collision (different name maps to same hash).
/// RwLock poisoning on collision panic is intentional — a collision
/// is fatal and the process should abort.
pub fn intern_keyword(name: &str) -> u64 {
    let ptr = INTERN_FN_PTR.load(Ordering::Acquire);
    if ptr != 0 {
        let intern: fn(&str) -> u64 = unsafe { std::mem::transmute(ptr) };
        return intern(name);
    }
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

/// Look up a keyword name by its 47-bit hash.
///
/// In the host binary, looks up in the local `KEYWORD_NAMES` table.
/// In cdylib plugins (after `set_keyword_fns` is called), delegates to the
/// host's `keyword_name` function to look up in the host's table.
///
/// Returns None only if the hash was never registered — should not happen
/// for any keyword created through Value::keyword().
pub fn keyword_name(hash: u64) -> Option<String> {
    let ptr = LOOKUP_FN_PTR.load(Ordering::Acquire);
    if ptr != 0 {
        let lookup: fn(u64) -> Option<String> = unsafe { std::mem::transmute(ptr) };
        return lookup(hash);
    }
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
        // Verify it's usable as a const — the specific value is deterministic
        // but not important; we just check it fits in 47 bits.
        assert_eq!(H & !((1u64 << 47) - 1), 0, "const hash must fit in 47 bits");
    }

    #[test]
    fn known_keywords_no_collision() {
        // Verify a representative corpus of Elle keywords don't collide.
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
