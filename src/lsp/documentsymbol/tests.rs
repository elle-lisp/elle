//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_document_symbols_lists_user_defs_excludes_synthetic() {
    crate::value::arena::with_test_region(|| {
        let uri = "file:///home/u/proj/s.lisp";
        // Two defs plus a bare top-level expression. The expression compiles to
        // a synthetic letrec wrapper binding, which must NOT surface as a symbol.
        let state = crate::lsp::testutil::compiled(uri, "(def a 1)\n(def b 2)\n(+ a b)\n");
        let doc = state.get_document(uri).unwrap();
        let syms = document_symbols(&doc.symbol_index);

        let names: Vec<&str> = syms
            .iter()
            .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(names.contains(&"a"), "got {names:?}");
        assert!(names.contains(&"b"), "got {names:?}");
        assert!(
            !names.iter().any(|n| n.starts_with("__file_expr")),
            "synthetic wrapper leaked: {names:?}"
        );

        // Every entry points at the document's own file.
        for s in &syms {
            assert_eq!(
                s.get("location")
                    .and_then(|l| l.get("uri"))
                    .and_then(|u| u.as_str()),
                Some(uri)
            );
        }
    });
}

#[test]
fn test_workspace_symbols_query_filters() {
    crate::value::arena::with_test_region(|| {
        let uri = "file:///home/u/proj/w.lisp";
        let state = crate::lsp::testutil::compiled(uri, "(def alpha 1)\n(def beta 2)\n");

        let all = workspace_symbols(state.document_indices(), "");
        assert!(all.len() >= 2, "empty query returns all, got {all:?}");

        let only = workspace_symbols(state.document_indices(), "alph");
        let names: Vec<&str> = only
            .iter()
            .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["alpha"]);
    });
}
