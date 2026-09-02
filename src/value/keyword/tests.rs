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

// The vocabulary is injective under the hash — the static half of the
// collision guard the memo enforces at run time.
#[test]
fn vocabulary_is_collision_free() {
    let mut seen = std::collections::HashMap::new();
    for name in VOCABULARY {
        if let Some(prev) = seen.insert(keyword_hash(name), name) {
            panic!("vocabulary collision: {:?} and {:?}", prev, name);
        }
    }
}

// Resolution order: the memo answers first, then the static vocabulary, then
// nothing. "error" is in the vocabulary; a fresh spelling is not.
#[test]
fn resolution_reads_memo_then_vocabulary() {
    let mut memo = crate::symbol::SymbolTable::new();
    let dynamic = memo.keyword("kw-resolve-order-xt");

    assert_eq!(
        resolve_keyword_name(Some(&memo), dynamic),
        Some("kw-resolve-order-xt"),
        "a learned spelling resolves through the memo"
    );
    assert_eq!(
        resolve_keyword_name(None, dynamic),
        None,
        "a run-time spelling is invisible without the memo that learned it"
    );
    assert_eq!(
        resolve_keyword_name(None, keyword_hash("error")),
        Some("error"),
        "a vocabulary spelling needs no memo"
    );
}

// Every fixed spelling the runtime mints must be in VOCABULARY, or the value
// it names prints as #<keyword:hash>. The scan covers the literal mint forms;
// spellings reaching keywords through enum accessors are covered by the
// corpus, which prints them.
//
// Counter-factual: remove "ok" from VOCABULARY and this fails on the
// `Value::keyword("ok")` sites.
#[test]
fn vocabulary_covers_literal_mint_sites() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = std::collections::BTreeSet::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable source dir") {
            let path = entry.expect("readable dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                // Test-only literals never reach a user's display path.
                && !path.to_string_lossy().contains("tests")
            {
                let text = std::fs::read_to_string(&path).expect("readable source file");
                for pat in ["Value::keyword(\"", "TableKey::keyword(\"", "ctx.error(\""] {
                    for (i, _) in text.match_indices(pat) {
                        let rest = &text[i + pat.len()..];
                        let name = &rest[..rest.find('"').expect("terminated literal")];
                        if static_keyword_name(keyword_hash(name)).is_none() {
                            missing.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "keyword spellings minted from literals but absent from VOCABULARY: {:?}",
        missing
    );
}
