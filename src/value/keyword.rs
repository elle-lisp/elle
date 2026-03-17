use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Global keyword name table. Maps 47-bit FNV-1a hash to keyword name.
/// Lives in the host binary's data segment. Shared with all cdylib plugins
/// that link against the same `elle` crate (verified empirically).
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

/// Look up a keyword name by its 47-bit hash.
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
