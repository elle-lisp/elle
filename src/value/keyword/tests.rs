//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn hash_is_deterministic() {
    assert_eq!(keyword_hash("foo"), keyword_hash("foo"));
    assert_eq!(keyword_hash("error"), keyword_hash("error"));
}

// Independent FNV-1a-64 oracle values, computed offline (not derived from
// this implementation). The hash must be the full, untruncated 64-bit
// FNV-1a digest. Each name below has nonzero bits above bit 47, so any
// reintroduced truncation would change these values and fail the test.
#[test]
fn hash_is_full_fnv1a_64() {
    assert_eq!(keyword_hash("foo"), 0x7eb4_f5db_4cbe_5ae7);
    assert_eq!(keyword_hash("error"), 0x1150_1d7d_839f_2da1);
    assert_eq!(
        keyword_hash("some-long-keyword-name-that-exercises-the-hash"),
        0xa401_ac10_2964_7995
    );
}

#[test]
fn hash_uses_bits_above_47() {
    // Guards against the old 47-bit truncation returning: these names
    // carry entropy in bits 47..64 that truncation would discard.
    assert_ne!(keyword_hash("foo") >> 47, 0);
    assert_ne!(keyword_hash("error") >> 47, 0);
    assert_ne!(keyword_hash("oxigraph-error") >> 47, 0);
}

#[test]
fn hash_is_const() {
    const H: u64 = keyword_hash("foo");
    assert_eq!(H, 0x7eb4_f5db_4cbe_5ae7);
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
