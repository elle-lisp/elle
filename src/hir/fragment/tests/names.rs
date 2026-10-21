// audited: 2026-10-21
//! The names a fragment answers: its binding table's, and the ones its body and patterns spell.
//!
//! docs/impl/region/template.md

use super::*;

/// Every symbol and every keyword spelling `fragment` answers, in walk order.
fn names_of(fragment: &HirFragment) -> (Vec<SymbolId>, Vec<String>) {
    let (mut symbols, mut keywords) = (Vec::new(), Vec::new());
    fragment.for_each_name(&mut |n| match n {
        FragmentName::Symbol(id) => symbols.push(id),
        FragmentName::Keyword(k) => keywords.push(k.to_string()),
    });
    (symbols, keywords)
}

// A carrier spells a fragment's names for another instance, and the binding
// table does not hold all of them: a keyword literal, a pattern's keyword and
// a struct pattern's keys sit in the body. The counter-factual is a walk over
// the binding table alone, which answers `x`, `a` and `b` and none of the four
// names the body spells.
#[test]
fn a_fragment_names_what_its_body_and_patterns_spell() {
    let l = lambda("(fn [x] (match x {:frag-key a 'frag-sym-key b} a :frag-lit 1 _ :frag-value))");
    let (_params, fragment) = l.close().expect("the body closes");

    let (symbols, keywords) = names_of(&fragment);
    for k in ["frag-key", "frag-lit", "frag-value"] {
        assert!(
            keywords.iter().any(|s| s == k),
            "the walk never answered :{k}, which the body spells"
        );
    }
    assert!(
        symbols.contains(&SymbolId::of("frag-sym-key")),
        "the walk never answered the struct pattern's symbol key"
    );
    for b in ["x", "a", "b"] {
        assert!(
            symbols.contains(&SymbolId::of(b)),
            "the walk never answered the binding {b}"
        );
    }
}
