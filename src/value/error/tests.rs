//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::value::arena::with_test_region;
use crate::value::types::sorted_struct_contains;

// One shared heap for all the error-builder helpers below, standing in for the
// VM's single per-instance heap: `tr()` mints regions on it and the `error_val*`
// wrappers build their structs on that same heap, so regions and values always
// agree. Leaked once per thread and reused for every helper call.
thread_local! {
    static TEST_HEAP: *mut crate::value::fiberheap::FiberHeap =
        crate::value::arena::leaked_test_heap();
}

fn test_heap_ptr() -> *mut crate::value::fiberheap::FiberHeap {
    TEST_HEAP.with(|p| *p)
}

/// Mint a fresh region on the shared (test) heap — the explicit region
/// `error_val_in`/`error_val_extra_in` take as an argument.
fn tr() -> RuntimeRegion {
    let heap_ptr = test_heap_ptr();
    unsafe { (*heap_ptr).new_runtime_region() }
}

/// Region-minting wrappers around the region-explicit error constructors. The
/// heap is the shared (test) heap, the same one `tr()` mints its region on.
fn error_val(kind: &str, msg: impl Into<String>) -> Value {
    let region = tr();
    error_val_in(unsafe { &mut *test_heap_ptr() }, kind, msg, region)
}
fn error_val_extra(kind: &str, msg: impl Into<String>, extra: &[(&str, Value)]) -> Value {
    let region = tr();
    error_val_extra_in(unsafe { &mut *test_heap_ptr() }, kind, msg, extra, region)
}

#[test]
fn test_error_val_creates_struct() {
    with_test_region(|| {
        let err = error_val("type-error", "expected integer");

        // Should be a struct
        assert!(err.as_struct().is_some());

        // Should have :error and :message keys
        let fields = err.as_struct().unwrap();
        assert!(sorted_struct_contains(
            fields,
            &TableKey::Keyword("error".into())
        ));
        assert!(sorted_struct_contains(
            fields,
            &TableKey::Keyword("message".into())
        ));

        // Values should be correct
        let error_key = sorted_struct_get(fields, &TableKey::Keyword("error".into())).unwrap();
        assert_eq!(error_key.as_keyword_name().as_deref(), Some("type-error"));

        let msg_key = sorted_struct_get(fields, &TableKey::Keyword("message".into())).unwrap();
        assert_eq!(
            msg_key.with_string(|s| s.to_string()),
            Some("expected integer".to_string())
        );
    })
}

#[test]
fn test_format_error_struct() {
    with_test_region(|| {
        let err = error_val("type-error", "expected integer");
        let formatted = format_error(err);
        assert_eq!(formatted, "type-error: expected integer");
    })
}

#[test]
fn test_format_error_legacy_array() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        // Legacy array error for backward compatibility
        let err = h.ctx().array(vec![
            Value::keyword("type-error"),
            h.ctx().string("expected integer"),
        ]);
        let formatted = format_error(err);
        assert_eq!(formatted, "type-error: expected integer");
    })
}

#[test]
fn test_format_error_plain_string() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let err = h.ctx().string("something went wrong");
        let formatted = format_error(err);
        assert_eq!(formatted, "something went wrong");
    })
}

#[test]
fn test_format_error_arbitrary_value() {
    with_test_region(|| {
        let err = Value::int(42);
        let formatted = format_error(err);
        // Should fall back to display representation
        assert_eq!(formatted, "42");
    })
}

#[test]
fn test_format_error_struct_with_string_message() {
    with_test_region(|| {
        let err = error_val("runtime-error", "division by zero");
        let formatted = format_error(err);
        assert_eq!(formatted, "runtime-error: division by zero");
    })
}

#[test]
fn test_error_val_extra_creates_struct() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let err = error_val_extra(
            "io-error",
            "slurp: failed to read '/no/such': file not found",
            &[("path", h.ctx().string("/no/such"))],
        );
        let fields = err.as_struct().unwrap();
        // :error keyword correct
        assert_eq!(
            sorted_struct_get(fields, &TableKey::Keyword("error".into()))
                .unwrap()
                .as_keyword_name()
                .as_deref(),
            Some("io-error"),
        );
        // :message correct
        assert!(sorted_struct_contains(
            fields,
            &TableKey::Keyword("message".into())
        ));
        // :path extra field present
        let path_val = sorted_struct_get(fields, &TableKey::Keyword("path".into())).unwrap();
        assert_eq!(
            path_val.with_string(|s| s.to_string()),
            Some("/no/such".to_string()),
        );
    })
}

#[test]
fn test_error_val_extra_empty_extras_matches_error_val() {
    with_test_region(|| {
        let a = error_val("type-error", "expected integer");
        let b = error_val_extra("type-error", "expected integer", &[]);
        // Both produce identical structs
        assert_eq!(a, b);
    })
}

#[test]
fn test_format_error_ignores_extra_fields() {
    with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let err = error_val_extra(
            "io-error",
            "slurp: failed to read '/nonexistent/x': not found",
            &[("path", h.ctx().string("/nonexistent/x"))],
        );
        let formatted = format_error(err);
        // format_error reads :error and :message; extra fields are silently ignored
        assert_eq!(
            formatted,
            "io-error: slurp: failed to read '/nonexistent/x': not found"
        );
    })
}
