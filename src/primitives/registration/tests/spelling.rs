// audited: 2026-11-20
//! Every spelling the build fixes hashes apart: primitive names, the keyword vocabulary, and the boot sources.
//!
//! docs/impl/symbol.md

use crate::symbol::SymbolTable;

// Identity is the name hash, and `static_index` and the keyword vocabulary are
// hash-keyed maps that keep the last spelling inserted on a hash, so a
// collision among the names the build fixes would silently make two names one.
// The memo's guard panics on a collision, so these tests record every
// build-fixed spelling into one memo (docs/impl/symbol.md).

/// The name and every alias of each primitive in the canonical tables.
fn record_primitive_spellings(memo: &mut SymbolTable) {
    for def in super::super::canonical_defs() {
        memo.intern(def.name);
        for alias in def.aliases {
            memo.intern(alias);
        }
    }
}

/// Every spelling in the keyword vocabulary.
fn record_vocabulary_spellings(memo: &mut SymbolTable) {
    for name in crate::value::keyword::VOCABULARY {
        memo.keyword(name);
    }
}

/// Every symbol and keyword token in one of the sources a boot compiles.
fn record_source_spellings(memo: &mut SymbolTable, source: &str) {
    let lexicon = crate::reader::lexicon_for(source).expect("a boot source's epoch");
    let mut lexer = crate::reader::Lexer::new(source).in_lexicon(lexicon);
    while let Some(token) = lexer.next_token_with_loc().expect("a boot source lexes") {
        match token.token {
            crate::reader::Token::Symbol(name) => {
                memo.intern(name);
            }
            crate::reader::Token::Keyword(name) => {
                memo.keyword(name);
            }
            _ => {}
        }
    }
}

/// Every spelling the build fixes.
fn record_static_spellings(memo: &mut SymbolTable) {
    use crate::pipeline::sources::{CORE, PRELUDE, STDLIB};
    record_primitive_spellings(memo);
    record_vocabulary_spellings(memo);
    for source in [CORE, PRELUDE, STDLIB] {
        record_source_spellings(memo, source);
    }
}

#[test]
fn static_vocabulary_is_collision_free() {
    let mut memo = SymbolTable::new();
    record_static_spellings(&mut memo);
    assert!(
        memo.len() > 1000,
        "only {} spellings recorded — the enumeration stopped reaching a source",
        memo.len()
    );
}

// The three tests below plant a second spelling on the hash of a name each
// source carries, and expect the recording to panic. Without them, a source the
// enumeration stopped reaching would leave the test above green over names it
// never checked.

/// A memo whose entry for `real`'s hash holds another spelling.
fn memo_with_an_impostor_on(real: &str) -> SymbolTable {
    let mut memo = SymbolTable::new();
    memo.record_spelling(crate::namehash::name_hash(real), "impostor-spelling-xt");
    memo
}

#[test]
#[should_panic(expected = "name hash collision")]
fn a_primitive_name_reaches_the_collision_check() {
    let first = super::super::ALL_TABLES[0][0].name;
    record_primitive_spellings(&mut memo_with_an_impostor_on(first));
}

#[test]
#[should_panic(expected = "name hash collision")]
fn a_vocabulary_spelling_reaches_the_collision_check() {
    let first = crate::value::keyword::VOCABULARY[0];
    record_vocabulary_spellings(&mut memo_with_an_impostor_on(first));
}

#[test]
#[should_panic(expected = "name hash collision")]
fn a_stdlib_symbol_reaches_the_collision_check() {
    use crate::pipeline::sources::STDLIB;
    record_source_spellings(&mut memo_with_an_impostor_on("defn"), STDLIB);
}
