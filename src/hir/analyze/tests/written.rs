// audited: 2026-11-24
//! What the analyzer marks as the source's own: the bindings it spells, the nodes built for its symbols, and one error per place.
//!
//! src/hir/analyze/overview.md

use super::*;

/// Whether a macro template introduced each binding named `name` that `result`
/// introduces, in tree order.
fn marks(result: &crate::pipeline::AnalyzeResult, name: &str) -> Vec<bool> {
    let mut introduced = Vec::new();
    result.hir.introduced_bindings(&mut introduced);
    introduced
        .into_iter()
        .map(|b| result.arena.get(b))
        .filter(|inner| inner.name == SymbolId::of(name))
        .map(|inner| inner.is_macro_introduced)
        .collect()
}

#[test]
fn the_bindings_each_introduces_are_marked_and_the_users_are_not() {
    // The counter-factual marked nothing, so `seq`, `at` and `broke` reached the
    // symbol list and the lint rules as names the user wrote.
    let result = analyze_file("(defn f [xs] (each x in xs x))");
    for name in ["seq", "given", "at", "broke"] {
        let marks = marks(&result, name);
        assert!(
            !marks.is_empty() && marks.iter().all(|m| *m),
            "{name} is the template's: {marks:?}"
        );
    }
    for name in ["f", "xs", "x"] {
        let marks = marks(&result, name);
        assert!(
            !marks.is_empty() && marks.iter().all(|m| !*m),
            "{name} is the user's: {marks:?}"
        );
    }
}

#[test]
fn a_name_datum_to_syntax_places_at_the_call_site_is_the_users() {
    // `datum->syntax` gives `it` the scopes of the caller's `test`, which carry
    // no intro scope. A mark read off the template would hide a name the caller
    // writes in the body.
    let result = analyze_file(
        "(defmacro aif (test then else)
           `(let [,(datum->syntax test 'it) ,test] (if ,test ,then ,else)))
         (defn f [a] (aif a it 0))",
    );
    let marks = marks(&result, "it");
    assert!(
        !marks.is_empty() && marks.iter().all(|m| !*m),
        "it is the caller's: {marks:?}"
    );
}

#[test]
fn the_written_set_holds_the_references_the_source_spells() {
    // Each copy of the body is text the user wrote, so every reference to `g`
    // is written. `type-of` is the template's, `array` is the constructor the
    // literal calls, and `get` is what `m:k` desugars to; none of them is.
    let result = analyze_file("(defn g [y] y)\n(defn f [xs m] (each x in xs (g x)) [m:k])");
    let named = |node: &Hir, name: &str| matches!(&node.kind, HirKind::Var(b) if result.arena.get(*b).name == SymbolId::of(name));
    let all = nodes(&result.hir);
    let g_refs: Vec<_> = all.iter().filter(|n| named(n, "g")).collect();
    assert!(!g_refs.is_empty(), "f refers to g");
    for node in &g_refs {
        assert!(
            result.written.contains(&node.id),
            "a reference to g is written"
        );
    }
    for name in ["type-of", "array", "get"] {
        assert!(
            !all.iter()
                .any(|n| named(n, name) && result.written.contains(&n.id)),
            "no reference to {name} is written"
        );
    }
    // The qualified name is written as one node: the outermost `get` call.
    let chains = all
        .iter()
        .filter(|n| {
            matches!(&n.kind, HirKind::Call { func, .. } if named(func, "get"))
                && result.written.contains(&n.id)
        })
        .count();
    assert_eq!(chains, 1, "m:k is one written node");
}

#[test]
fn an_undefined_name_each_copy_of_a_body_repeats_is_one_error() {
    // The counter-factual collected one error for each copy of the body, and
    // `compile/diagnostics` reported the one typo four times.
    let result = analyze_file("(defn f [xs] (each x in xs (nope x)))");
    assert_eq!(result.errors.len(), 1, "errors: {:?}", result.errors);
}
