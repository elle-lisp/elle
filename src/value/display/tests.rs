//! Unit tests (`super` is the parent impl module).

use crate::value::arena::with_test_region;
use crate::value::error::error_val_in;
use crate::value::Value;

/// Debug repr of a string containing a double-quote must escape it.
#[test]
fn test_debug_string_escapes_double_quote() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let val = h.ctx().string("say \"hello\"");
        let repr = format!("{:?}", val);
        assert_eq!(repr, r#""say \"hello\"""#);
    })
}

/// Debug repr of a string containing a backslash must escape it.
#[test]
fn test_debug_string_escapes_backslash() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let val = h.ctx().string("path\\to\\file");
        let repr = format!("{:?}", val);
        assert_eq!(repr, r#""path\\to\\file""#);
    })
}

/// Debug repr of a string containing both backslash and double-quote.
/// Backslash must be escaped before quote (order matters).
#[test]
fn test_debug_string_escapes_backslash_and_quote() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let val = h.ctx().string("a\\\"b");
        let repr = format!("{:?}", val);
        assert_eq!(repr, r#""a\\\"b""#);
    })
}

/// Display of a struct with a string value must quote and escape the string.
#[test]
fn test_display_struct_quotes_string_values() {
    with_test_region(|| {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let err = error_val_in(
            unsafe { &mut *heap_ptr },
            "type-error",
            "expected \"integer\"",
            region,
        );
        let repr = format!("{}", err);
        // The struct has :error and :message keys (BTreeMap, sorted by key).
        // :error → :type-error (keyword, no quotes)
        // :message → "expected \"integer\"" (string, quoted and escaped)
        assert!(repr.contains(r#""expected \"integer\"""#), "got: {}", repr);
    })
}

// ── keyword display resolves memo → vocabulary → unreadable form ─────

// A vocabulary spelling ("ok") needs no memo; a run-time spelling resolves
// only through the memo that learned it; an unlearned spelling renders the
// unreadable form. The form is deliberately not a keyword literal: any
// `:something` rendering would denote a real — and different — keyword.
#[test]
fn keyword_display_resolves_memo_then_vocabulary_then_unreadable() {
    let mut memo = crate::symbol::SymbolTable::new();
    memo.keyword("kw-display-learned-xt");

    let learned = Value::keyword("kw-display-learned-xt");
    let vocab = Value::keyword("ok");
    let unlearned = Value::keyword("kw-display-unlearned-xt");

    assert_eq!(
        format!("{}", learned.display_with(Some(&memo))),
        ":kw-display-learned-xt"
    );
    assert_eq!(format!("{}", vocab), ":ok");
    assert_eq!(
        format!("{}", unlearned.display_with(Some(&memo))),
        format!(
            "#<keyword:{:#x}>",
            crate::value::keyword::keyword_hash("kw-display-unlearned-xt")
        )
    );
}
