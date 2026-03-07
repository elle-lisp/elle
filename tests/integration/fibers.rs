// Integration tests for fiber primitives (FiberHandle, child chain, propagate, cancel)
//
// This file contains only tests that verify error message content.
// Behavioral tests have been migrated to tests/elle/fibers.lisp.

use crate::common::eval_source;

// ── fiber/propagate ──────────────────────────────────────────────

#[test]
fn test_fiber_propagate_error() {
    // Create a fiber that errors, catch it, then propagate
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 1 "boom")) 1)))
          (fiber/resume inner)
          (fiber/propagate inner))
        "#,
    );
    // The propagated error should surface as an error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("boom"), "Expected 'boom' in error: {}", err);
}

#[test]
fn test_fiber_propagate_dead_fiber_errors() {
    // Propagating from a dead (completed) fiber should error
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 0)))
          (fiber/resume f)
          (fiber/propagate f))
        "#,
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("errored or suspended"),
        "Expected status error, got: {}",
        err
    );
}
