//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_find_references_returns_empty_for_empty_index() {
    let index = SymbolIndex::new();

    let references = find_references(0, 0, false, &index);
    assert!(references.is_empty());
}

#[test]
fn test_find_references_out_of_range_position() {
    let index = SymbolIndex::new();

    let references = find_references(1000, 1000, false, &index);
    assert!(references.is_empty());
}

#[test]
fn test_find_references_collects_usages_and_declaration() {
    crate::value::arena::with_test_region(|| {
        let uri = "file:///home/u/proj/r.lisp";
        // `foo` used twice on line 2.
        let state = crate::lsp::testutil::compiled(uri, "(def foo 1)\n(+ foo foo)\n");
        let doc = state.get_document(uri).unwrap();

        // From the definition site (line 0, char 5), without the declaration.
        let refs = find_references(0, 5, false, &doc.symbol_index);
        assert_eq!(refs.len(), 2, "two usages expected, got {refs:?}");
        for r in &refs {
            assert_eq!(r.get("uri").and_then(|u| u.as_str()), Some(uri));
        }

        // Including the declaration adds the definition site.
        let with_decl = find_references(0, 5, true, &doc.symbol_index);
        assert_eq!(
            with_decl.len(),
            3,
            "usages + declaration, got {with_decl:?}"
        );
    });
}
