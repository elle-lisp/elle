//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_compiler_state_creation() {
    crate::value::arena::with_test_region(|| {
        let state = CompilerState::new();
        assert_eq!(state.documents.len(), 0);
    });
}

/// True if the document's index records a definition named `name`.
fn has_def(state: &CompilerState, uri: &str, name: &str) -> bool {
    state
        .get_document(uri)
        .map(|d| d.symbol_index.definitions.values().any(|s| s.name == name))
        .unwrap_or(false)
}

// Isolation spec: a macro defined in one version of a document must NOT keep
// expanding after it is removed from the source on the next compile. The macro
// expands to a `def`, so a leak is observable: if `defthing` survived, the v2
// `(defthing bar)` would still expand and the index would record `bar`.
#[test]
fn macro_does_not_leak_across_recompile() {
    crate::value::arena::with_test_region(|| {
        let mut state = CompilerState::new();
        let uri = "file:///home/u/proj/m.lisp";
        state.on_document_open(
            uri.to_string(),
            "(defmacro defthing (n) `(def ,n 42))\n(defthing foo)\n".to_string(),
        );
        state.compile_document(uri);
        assert!(
            has_def(&state, uri, "foo"),
            "v1 macro should expand to def foo"
        );

        // v2 removes the macro definition but still calls it.
        state.on_document_change(uri, "(defthing bar)\n".to_string());
        state.compile_document(uri);
        assert!(
            !has_def(&state, uri, "bar"),
            "macro leaked across recompile: defthing still expanded after removal"
        );
    });
}

// Isolation spec: a macro defined in document A must NOT be visible to document
// B sharing the same CompilerState.
#[test]
fn macro_does_not_leak_across_documents() {
    crate::value::arena::with_test_region(|| {
        let mut state = CompilerState::new();
        let a = "file:///home/u/proj/a.lisp";
        let b = "file:///home/u/proj/b.lisp";
        state.on_document_open(
            a.to_string(),
            "(defmacro mkdef (n) `(def ,n 1))\n(mkdef in-a)\n".to_string(),
        );
        state.compile_document(a);
        assert!(
            has_def(&state, a, "in-a"),
            "A's macro should expand within A"
        );

        state.on_document_open(b.to_string(), "(mkdef leaked)\n".to_string());
        state.compile_document(b);
        assert!(
            !has_def(&state, b, "leaked"),
            "macro leaked from document A into document B"
        );
    });
}

#[test]
fn test_document_open_and_close() {
    crate::value::arena::with_test_region(|| {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "(+ 1 2)".to_string());
        assert_eq!(state.documents.len(), 1);

        state.on_document_close("file:///test.l");
        assert_eq!(state.documents.len(), 0);
    });
}

#[test]
fn test_document_change() {
    crate::value::arena::with_test_region(|| {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "(+ 1 2)".to_string());

        if let Some(doc) = state.documents.get("file:///test.l") {
            assert_eq!(doc.source_text, "(+ 1 2)");
        }

        state.on_document_change("file:///test.l", "(+ 3 4)".to_string());
        if let Some(doc) = state.documents.get("file:///test.l") {
            assert_eq!(doc.source_text, "(+ 3 4)");
        }
    });
}

#[test]
fn test_compile_simple_expression() {
    crate::value::arena::with_test_region(|| {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "(+ 1 2)".to_string());
        let result = state.compile_document("file:///test.l");
        assert!(result);
    });
}

#[test]
fn test_extract_location_from_error() {
    // Standard reader error format
    let loc = extract_location_from_error("<lsp>:1:4: unterminated list");
    assert!(loc.is_some());
    let loc = loc.unwrap();
    assert_eq!(loc.file, "<lsp>");
    assert_eq!(loc.line, 1);
    assert_eq!(loc.col, 4);

    // Multi-digit line/col
    let loc = extract_location_from_error("<lsp>:12:34: some error");
    assert!(loc.is_some());
    let loc = loc.unwrap();
    assert_eq!(loc.line, 12);
    assert_eq!(loc.col, 34);
}

#[test]
fn test_extract_location_from_error_invalid() {
    // No angle brackets
    assert!(extract_location_from_error("something went wrong").is_none());
    // Missing colon-separated parts
    assert!(extract_location_from_error("<lsp>: message").is_none());
}

#[test]
fn test_compile_syntax_error_has_location() {
    crate::value::arena::with_test_region(|| {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "((((".to_string());
        state.compile_document("file:///test.l");
        let doc = state.get_document("file:///test.l").unwrap();
        assert!(!doc.diagnostics.is_empty());
        let diag = &doc.diagnostics[0];
        assert!(
            diag.location.is_some(),
            "parse error diagnostic should have a location"
        );
        let loc = diag.location.as_ref().unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.col, 4);
    });
}
