//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_find_definition_returns_none_for_empty_index() {
    let index = SymbolIndex::new();

    let definition = find_definition(0, 0, &index);
    assert!(definition.is_none());
}

#[test]
fn test_definition_points_to_real_file_uri() {
    crate::value::arena::with_test_region(|| {
        let uri = "file:///home/u/proj/d.lisp";
        // `foo` defined on line 1, used on line 2.
        let state = crate::lsp::testutil::compiled(uri, "(def foo 1)\n(+ foo foo)\n");
        let doc = state.get_document(uri).unwrap();
        // Jump from the usage of `foo` (line 1, char 3) to its definition.
        let def =
            find_definition(1, 3, &doc.symbol_index).expect("usage should resolve to a definition");
        // The URI must be the document's own file, not the old `file://<unknown>`.
        assert_eq!(def.get("uri").and_then(|u| u.as_str()), Some(uri));
        let start = def.get("range").and_then(|r| r.get("start")).unwrap();
        assert_eq!(start.get("line").and_then(|l| l.as_u64()), Some(0));
    });
}
