// audited: 2026-09-30
//! Tests for applying byte-span edits to source text.

use super::*;

/// The edit that replaces `len` bytes at `offset` with `text`.
fn edit(offset: usize, len: usize, text: &str) -> Edit {
    Edit {
        byte_offset: offset,
        byte_len: len,
        replacement: text.to_string(),
    }
}

#[test]
fn test_single_edit() {
    let source = "(path/join a b)";
    let mut edits = vec![edit(1, 9, "path-join")];
    assert_eq!(apply_edits(source, &mut edits).unwrap(), "(path-join a b)");
}

#[test]
fn test_multiple_edits() {
    let source = "(path/join (path/parent x))";
    let mut edits = vec![edit(1, 9, "path-join"), edit(12, 11, "path-parent")];
    assert_eq!(
        apply_edits(source, &mut edits).unwrap(),
        "(path-join (path-parent x))"
    );
}

#[test]
fn test_empty_edits() {
    let source = "(+ 1 2)";
    let mut edits: Vec<Edit> = vec![];
    assert_eq!(apply_edits(source, &mut edits).unwrap(), "(+ 1 2)");
}

#[test]
fn test_different_length_replacement() {
    let source = "(fn/arity f)";
    let mut edits = vec![edit(1, 8, "fn-arity")];
    assert_eq!(apply_edits(source, &mut edits).unwrap(), "(fn-arity f)");
}

#[test]
fn test_overlapping_edits_error() {
    let source = "abcdefgh";
    let mut edits = vec![edit(2, 4, "X"), edit(4, 3, "Y")];
    let result = apply_edits(source, &mut edits);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("overlapping edits"));
}

#[test]
fn an_insertion_and_a_replacement_at_one_offset_both_apply() {
    // A migration opens a wrap where a form starts, and a rename may replace
    // that form's first token. The insertion belongs before the new text,
    // whichever order the passes produced the two edits in.
    let source = "(f old)";
    let wrap_open = edit(3, 0, "(g ");
    let rename = edit(3, 3, "new");
    let wrap_close = edit(6, 0, ")");
    for mut edits in [
        vec![wrap_open.clone(), rename.clone(), wrap_close.clone()],
        vec![rename.clone(), wrap_open.clone(), wrap_close.clone()],
    ] {
        assert_eq!(apply_edits(source, &mut edits).unwrap(), "(f (g new))");
    }
}
