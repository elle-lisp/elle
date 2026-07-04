//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_hover_info_returns_none_for_empty_index() {
    let index = SymbolIndex::new();
    let docs = HashMap::new();

    let hover = find_hover_info(0, 0, &index, &docs);
    assert!(hover.is_none());
}

#[test]
fn test_hover_reports_user_function() {
    crate::value::arena::with_test_region(|| {
        let uri = "file:///home/u/proj/h.lisp";
        let state = crate::lsp::testutil::compiled(uri, "(def square (fn [x] (* x x)))\n");
        let doc = state.get_document(uri).unwrap();
        // Cursor on `square` (line 0, char 6).
        let hover = find_hover_info(0, 6, &doc.symbol_index, state.docs())
            .expect("hover over a definition should resolve");
        let contents = hover.get("contents").and_then(|c| c.as_array()).unwrap();
        let joined: String = contents
            .iter()
            .filter_map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Function"), "got: {joined}");
    });
}
