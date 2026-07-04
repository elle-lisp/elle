//! Unit tests (`super` is the parent impl module).

use crate::value::arena::with_test_region;
use crate::value::error::error_val_in;

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
