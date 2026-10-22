// audited: 2026-10-12
//! A qualified symbol teaches the memo the keys its desugaring names.
//!
//! docs/impl/symbol.md

use super::*;
use crate::value::keyword::keyword_hash;

// `m:quokka:wombat` desugars to `(get (get m :quokka) :wombat)`, and no keyword
// token spells either key. The counter-factual is a desugaring that builds the
// keywords and records nothing: the program still runs, and each key prints as
// `#<keyword:hash>` — unless the boot happened to read the spelling elsewhere,
// which is what made a source boot and an image boot render one bytecode two
// ways. The names are ones no boot source spells, and the memo is fresh.
#[test]
fn a_qualified_symbol_records_the_spelling_of_each_key() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    let syntax = make_list(vec![
        make_symbol("fn"),
        make_array(vec![make_symbol("m")]),
        make_symbol("m:quokka:wombat"),
    ]);
    analyzer.analyze(&syntax).expect("analyzes");
    for key in ["quokka", "wombat"] {
        assert_eq!(
            symbols.keyword_name(keyword_hash(key)),
            Some(key),
            "the memo never learned :{key}"
        );
    }
}
