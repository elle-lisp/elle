//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_validate_new_name_empty() {
    assert!(validate_new_name("").is_err());
}

#[test]
fn test_validate_new_name_reserved_word() {
    assert!(validate_new_name("def").is_err());
    assert!(validate_new_name("var").is_err());
    assert!(validate_new_name("if").is_err());
}

#[test]
fn test_validate_new_name_invalid_characters() {
    assert!(validate_new_name("foo@bar").is_err());
    assert!(validate_new_name("foo bar").is_err());
}

#[test]
fn test_validate_new_name_valid() {
    assert!(validate_new_name("my-function").is_ok());
    assert!(validate_new_name("my_function").is_ok());
    assert!(validate_new_name("myFunction").is_ok());
    assert!(validate_new_name("my123").is_ok());
}

#[test]
fn test_rename_symbol_no_symbol_at_position() {
    let index = SymbolIndex::new();
    let symbol_table = SymbolTable::new();
    let uri = "file:///test.elle";

    let result = rename_symbol(0, 0, "bar", &index, &symbol_table, uri);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No symbol found at the given position"));
}

#[test]
fn test_rename_symbol_empty_index_errors() {
    let index = SymbolIndex::new();
    let symbol_table = SymbolTable::new();
    let uri = "file:///test.elle";

    let result = rename_symbol(0, 10, "bar", &index, &symbol_table, uri);
    assert!(result.is_err());
}

#[test]
fn test_check_rename_conflict_no_conflict() {
    let index = SymbolIndex::new();
    let symbol_table = SymbolTable::new();

    let result = check_rename_conflict("foo", "bar", &index, &symbol_table);
    assert!(result.is_ok());
}

/// Regression: renaming a top-level def must produce a non-empty WorkspaceEdit
/// whose edits target the document's own URI. This exercises the full path with
/// an index built from real source (the existing tests all used an empty index,
/// which is why the file-URI bug shipped: every edit was filtered out because
/// the index recorded `<unknown>` as the file).
#[test]
fn test_rename_top_level_def_produces_edits() {
    crate::value::arena::with_test_region(|| {
        let uri = "file:///home/u/proj/foo.lisp";
        // `foo` defined on line 1, used twice on line 2.
        let src = "(def foo 1)\n(+ foo foo)\n";
        let state = crate::lsp::testutil::compiled(uri, src);
        let doc = state.get_document(uri).unwrap();

        // Cursor on the definition `foo` (line 0, char 5 — 0-based).
        let result = rename_symbol(0, 5, "bar", &doc.symbol_index, state.symbol_table(), uri)
            .expect("rename should succeed");

        let changes = result
            .get("changes")
            .and_then(|c| c.as_object())
            .expect("workspace edit has a changes map");
        let edits = changes
            .get(uri)
            .and_then(|e| e.as_array())
            .unwrap_or_else(|| panic!("expected edits keyed by {uri}, got {changes:?}"));
        // def + two usages = 3 edits.
        assert_eq!(edits.len(), 3, "expected 3 edits, got {edits:?}");
        for e in edits {
            assert_eq!(e.get("newText").and_then(|t| t.as_str()), Some("bar"));
        }
    });
}
