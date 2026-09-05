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

// `vocab` is the compile-time half of the guard, and the only half that
// reaches a spelling written as a *token* rather than a string — a
// `rich_error!` field name, which becomes a keyword through `stringify!`. A
// `const` block calling it with a spelling the vocabulary lacks is a build
// error at the line that wrote it.
//
// A crate cannot assert that it fails to compile, so what is pinned here is
// the predicate the assertion rests on, and that it answers in a const
// context at all — a `vocab` that stopped being `const fn` would still pass
// every other test in this file while the guard silently moved to run time.
#[test]
fn vocabulary_membership_answers_in_a_const_context() {
    const CHECKED: &str = vocab("message");
    assert_eq!(CHECKED, "message");

    assert!(is_vocabulary("error"), "a listed spelling is carried");
    assert!(
        !is_vocabulary("kw-not-in-the-vocabulary-xt"),
        "an unlisted spelling is not"
    );
}

// Every form in `src/` that hands a *literal* spelling to a keyword
// constructor. `kw("…")` is the per-module helper each primitive module
// defines for its struct keys (`primitives/compile/mod.rs`,
// `primitives/fileio/manage.rs`) and the reader's syntax builder
// (`reader/synbuild.rs`); it is the form the earlier scan missed, and the form
// most of the runtime's struct keys are written in. `ctx.error("…")` and
// `io_error("…")` name a kind that becomes the `:error` field's keyword.
// `ctx.external("…")` names a type that becomes the keyword `type-of` returns.
const LITERAL_MINT_FORMS: &[&str] = &[
    "Value::keyword(\"",
    "TableKey::keyword(\"",
    "kw(\"",
    "ctx.error(\"",
    "io_error(\"",
    "ctx.external(\"",
];

// Every fixed spelling the runtime mints must be in VOCABULARY, or the value
// it names prints as #<keyword:hash> — and `json/serialize` refuses a struct
// that carries it as a key (docs/impl/symbol.md § "A spelling the runtime
// itself mints").
//
// Counter-factual: remove "ok" from VOCABULARY and this fails on the
// `Value::keyword("ok")` sites; remove "size" and it fails on `file/stat`'s
// `kw("size")`.
#[test]
fn vocabulary_covers_literal_mint_sites() {
    let mut missing = std::collections::BTreeMap::new();
    for (path, text) in runtime_sources() {
        for pat in LITERAL_MINT_FORMS {
            for (i, _) in text.match_indices(pat) {
                let rest = &text[i + pat.len()..];
                let name = &rest[..rest.find('"').expect("terminated literal")];
                if static_keyword_name(keyword_hash(name)).is_none() {
                    missing.insert(name.to_string(), path.clone());
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

// The other half: a spelling that reaches the same constructors through a
// `&'static str` an accessor returns. The literal sits in a `match` arm, not
// in the call, so the scan above cannot see it — every one of these tables
// drifted out from under VOCABULARY without a single test noticing.
//
// Each table is enumerated rather than listed, so adding a variant fails this
// test until the spelling is added to VOCABULARY too.
//
// Counter-factual: remove "alive" from VOCABULARY and this fails on
// `FiberStatus::Alive`.
#[test]
fn vocabulary_covers_accessor_mint_sites() {
    use crate::config::{JitPolicy, MlirPolicy, WasmPolicy};
    use crate::io::watch::WatchEventKind;
    use crate::value::fiber::FiberStatus;

    let mut spellings: Vec<(&str, String)> = Vec::new();

    // The signal registry's built-ins: `(signals)`, `fiber/caps`, and a
    // capability denial all mint a keyword per entry name.
    for entry in crate::signals::registry::SignalRegistry::with_builtins().entries() {
        spellings.push(("signal registry", entry.name.clone()));
    }

    // The POSIX signal names `os/sig-watch` and friends hand back. The table
    // is private, so walk the signum space it maps.
    for signum in 1..=64 {
        if let Some(name) = crate::io::sigmap::signum_to_keyword(signum) {
            spellings.push(("sigmap", name.to_string()));
        }
    }

    // The tier policies `(vm/config)` reports.
    for p in [
        JitPolicy::Off,
        JitPolicy::Eager,
        JitPolicy::Adaptive { threshold: 1 },
        JitPolicy::Custom,
    ] {
        spellings.push(("JitPolicy", p.keyword().to_string()));
    }
    for p in [
        WasmPolicy::Off,
        WasmPolicy::Full,
        WasmPolicy::Lazy { threshold: 1 },
    ] {
        spellings.push(("WasmPolicy", p.keyword().to_string()));
    }
    for p in [
        MlirPolicy::Off,
        MlirPolicy::Eager,
        MlirPolicy::Adaptive { threshold: 1 },
    ] {
        spellings.push(("MlirPolicy", p.keyword().to_string()));
    }

    // The fiber status `fiber/status` hands back.
    for s in [
        FiberStatus::New,
        FiberStatus::Alive,
        FiberStatus::Paused,
        FiberStatus::Dead,
        FiberStatus::Error,
    ] {
        spellings.push(("FiberStatus", s.as_str().to_string()));
    }

    // The file-watch event kind each `:kind` field carries.
    for k in [
        WatchEventKind::Create,
        WatchEventKind::Modify,
        WatchEventKind::Remove,
        WatchEventKind::Rename,
    ] {
        spellings.push(("WatchEventKind", k.as_keyword().to_string()));
    }

    // `VM::active_tier` is a bare `&'static str` field, so its values are
    // assignments rather than a table. Anchor on the field name and take
    // every string literal on the lines that write it.
    for (path, text) in runtime_sources() {
        if !path.starts_with("vm") {
            continue;
        }
        for line in text.lines() {
            if !line.contains("active_tier") {
                continue;
            }
            for (i, _) in line.match_indices('"') {
                let rest = &line[i + 1..];
                if let Some(end) = rest.find('"') {
                    spellings.push(("active_tier", rest[..end].to_string()));
                }
            }
        }
    }

    let missing: std::collections::BTreeSet<String> = spellings
        .iter()
        .filter(|(_, name)| static_keyword_name(keyword_hash(name)).is_none())
        .map(|(table, name)| format!("{table}: {name}"))
        .collect();
    assert!(
        missing.is_empty(),
        "keyword spellings minted from accessors but absent from VOCABULARY: {:?}",
        missing
    );
    assert!(
        spellings.len() > 40,
        "the accessor tables went empty — the enumeration stopped reaching them"
    );
}

/// Every non-test `.rs` file under `src/`, as (path relative to `src/`, text).
/// Test-only literals never reach a user's display path, so they are excluded.
fn runtime_sources() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable source dir") {
            let path = entry.expect("readable dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path.to_string_lossy().contains("tests")
            {
                let rel = path
                    .strip_prefix(&root)
                    .expect("a source under src/")
                    .to_string_lossy()
                    .into_owned();
                out.push((
                    rel,
                    std::fs::read_to_string(&path).expect("readable source"),
                ));
            }
        }
    }
    out
}
