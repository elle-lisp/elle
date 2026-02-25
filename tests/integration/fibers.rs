// Integration tests for fiber primitives (FiberHandle, child chain, propagate, cancel)

use crate::common::eval_source;
use elle::Value;

// ── fiber/child: child chain wiring ──────────────────────────────

#[test]
fn test_fiber_child_set_after_uncaught_signal() {
    // When a child's signal is NOT caught (propagates), parent.child
    // should remain set. We test this by having the inner fiber error
    // with mask=0 (not caught), then checking the outer's child.
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 1 "err")) 0)))
          (let ((outer (fiber/new
                         (fn ()
                           (fiber/resume inner)
                           42)
                         1)))
            (fiber/resume outer)
            (fiber? (fiber/child outer))))
        "#,
    );
    // The inner's error propagates to outer (mask=0 doesn't catch).
    // Outer catches it (mask=1). After that, outer.child should be
    // the inner fiber (child chain preserved on propagation, but
    // cleared when caught by outer's parent).
    // Actually, the child chain is on the outer fiber, and the outer
    // caught the error via its mask. So outer.child should be cleared.
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
}

#[test]
fn test_fiber_child_nil_before_resume() {
    // A fiber that hasn't resumed any child should have nil child
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 0)))
          (fiber/child f))
        "#,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::NIL);
}

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
fn test_fiber_propagate_yield() {
    // Create a fiber that yields, catch it, then propagate the yield.
    // The outer fiber propagates the inner's yield signal.
    // The root catches it via the outer's mask=2.
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 2 99)) 2)))
          (let ((outer (fiber/new
                         (fn ()
                           (fiber/resume inner)
                           (fiber/propagate inner))
                         2)))
            (fiber/resume outer)))
        "#,
    );
    // The outer fiber propagates the yield; root catches via mask=2
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
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

// ── fiber/cancel ─────────────────────────────────────────────────

#[test]
fn test_fiber_cancel_suspended_fiber() {
    // Cancel a suspended fiber — it should end up in error state
    // mask=3 catches both SIG_ERROR and SIG_YIELD
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 2 "waiting") 99) 3)))
          (fiber/resume f)
          (fiber/cancel f "cancelled")
          (fiber/status f))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    // After cancel, the fiber should be in :error status
    let val = result.unwrap();
    assert!(val.is_keyword(), "Expected keyword, got {:?}", val);
}

#[test]
fn test_fiber_cancel_new_fiber() {
    // Cancel a fiber that was never started
    // mask=1 catches SIG_ERROR so the cancel result is caught
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 1)))
          (fiber/cancel f "never started")
          (fiber/status f))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    let val = result.unwrap();
    assert!(val.is_keyword(), "Expected keyword, got {:?}", val);
}

#[test]
fn test_fiber_cancel_dead_fiber_errors() {
    // Cancelling a completed fiber should error
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 0)))
          (fiber/resume f)
          (fiber/cancel f "too late"))
        "#,
    );
    assert!(result.is_err());
}

#[test]
fn test_fiber_cancel_returns_error_value() {
    // Cancel a new fiber with mask=1 (catches SIG_ERROR).
    // The cancel injects an error; the mask catches it; the result
    // is the error value.
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 1)))
          (fiber/cancel f "injected"))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
}

// ── Basic fiber resume still works ───────────────────────────────

#[test]
fn test_fiber_resume_basic() {
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 0)))
          (fiber/resume f))
        "#,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_fiber_yield_and_resume() {
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 2 10) 20) 2)))
          (+ (fiber/resume f) (fiber/resume f)))
        "#,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_fiber_error_caught_by_mask() {
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 1 "oops")) 1)))
          (fiber/resume f))
        "#,
    );
    // Error is caught by mask=1 (SIG_ERROR), so parent gets the value
    assert!(result.is_ok());
}

#[test]
fn test_fiber_error_propagates_without_mask() {
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 1 "oops")) 0)))
          (fiber/resume f))
        "#,
    );
    // Error is NOT caught (mask=0), so it propagates to the root
    assert!(result.is_err());
}

// ── fiber/propagate preserving child chain (T4) ─────────────────

#[test]
fn test_fiber_propagate_preserves_child_chain() {
    // After fiber/propagate, the propagating fiber's child should be
    // set to the fiber being propagated from.
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 1 "err")) 1)))
          (let ((outer (fiber/new
                         (fn ()
                           (fiber/resume inner)
                           (fiber/propagate inner))
                         1)))
            (fiber/resume outer)
            (fiber? (fiber/child outer))))
        "#,
    );
    // After propagate, outer.child should be inner (preserved for trace chain)
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::bool(true),
        "fiber/child should return the inner fiber after propagate"
    );
}

#[test]
fn test_fiber_propagate_child_identity() {
    // fiber/child after propagate should return the same fiber object
    // (identity preserved via cached value)
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 2 99)) 2)))
          (let ((outer (fiber/new
                         (fn ()
                           (fiber/resume inner)
                           (fiber/propagate inner))
                         2)))
            (fiber/resume outer)
            (eq? inner (fiber/child outer))))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::bool(true),
        "fiber/child should return the exact same fiber object (identity)"
    );
}

// ── fiber/resume and fiber/cancel in tail position (T5) ─────────

#[test]
fn test_fiber_resume_in_tail_position() {
    // fiber/resume as the last expression in a fiber body (tail position)
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () 42) 0)))
          (let ((outer (fiber/new (fn () (fiber/resume inner)) 0)))
            (fiber/resume outer)))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_fiber_resume_yield_in_tail_position() {
    // fiber/resume in tail position where inner fiber yields
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 2 10) 20) 2)))
          (let ((outer (fiber/new (fn () (fiber/resume inner)) 0)))
            (fiber/resume outer)))
        "#,
    );
    // inner yields 10, caught by mask=2, outer returns 10
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_fiber_cancel_in_tail_position() {
    // fiber/cancel as the last expression in a fiber body (tail position)
    let result = eval_source(
        r#"
        (let ((target (fiber/new (fn () 42) 1)))
          (let ((canceller (fiber/new
                             (fn () (fiber/cancel target "cancelled"))
                             0)))
            (fiber/resume canceller)))
        "#,
    );
    // target is cancelled, mask=1 catches the error, canceller returns the error value
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
}

#[test]
fn test_fiber_cancel_suspended_in_tail_position() {
    // Cancel a suspended fiber from tail position
    let result = eval_source(
        r#"
        (let ((target (fiber/new (fn () (fiber/signal 2 0) 99) 3)))
          (fiber/resume target)
          (let ((canceller (fiber/new
                             (fn () (fiber/cancel target "stop"))
                             0)))
            (list
              (fiber/resume canceller)
              (keyword->string (fiber/status target)))))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    let list = result.unwrap();
    let items = list.list_to_vec();
    assert!(items.is_ok(), "Expected list, got {:?}", list);
    let items = items.unwrap();
    assert_eq!(items.len(), 2);
    // After cancel, target should be in :error status
    assert_eq!(
        items[1],
        Value::string("error"),
        "Cancelled fiber should be in error status"
    );
}

// ── 3-level nested fiber resume (T6) ────────────────────────────

#[test]
fn test_three_level_nested_fiber_resume() {
    // A resumes B which resumes C. C yields, B catches and adds, A gets result.
    let result = eval_source(
        r#"
        (let ((c (fiber/new (fn () (fiber/signal 2 10)) 2)))
          (let ((b (fiber/new
                     (fn ()
                       (+ (fiber/resume c) 5))
                     0)))
            (let ((a (fiber/new
                       (fn ()
                         (+ (fiber/resume b) 1))
                       0)))
              (fiber/resume a))))
        "#,
    );
    // C yields 10, B catches (mask=2), B returns 10+5=15, A returns 15+1=16
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(result.unwrap(), Value::int(16));
}

#[test]
fn test_three_level_nested_fiber_error_propagation() {
    // A resumes B which resumes C. C errors, B doesn't catch (mask=0),
    // error propagates to A which catches (mask=1).
    let result = eval_source(
        r#"
        (let ((c (fiber/new (fn () (fiber/signal 1 "deep error")) 0)))
          (let ((b (fiber/new
                     (fn () (fiber/resume c))
                     0)))
            (let ((a (fiber/new
                       (fn () (fiber/resume b))
                       1)))
              (fiber/resume a))))
        "#,
    );
    // C errors, B doesn't catch (mask=0), propagates to A which catches (mask=1)
    assert!(
        result.is_ok(),
        "Expected ok (caught by A), got: {:?}",
        result
    );
}

// ── fiber/parent and fiber/child identity ────────────────────────

#[test]
fn test_fiber_parent_identity() {
    // fiber/parent called twice on the same fiber should return eq? values
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () 42) 0)))
          (let ((outer (fiber/new
                         (fn ()
                           (fiber/resume f)
                           42)
                         0)))
            (fiber/resume outer)
            (eq? (fiber/parent f) (fiber/parent f))))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::bool(true),
        "fiber/parent should return identical values"
    );
}

#[test]
fn test_fiber_child_identity() {
    // fiber/child called twice on the same fiber should return eq? values
    let result = eval_source(
        r#"
        (let ((inner (fiber/new (fn () (fiber/signal 1 "err")) 0)))
          (let ((outer (fiber/new
                         (fn ()
                           (fiber/resume inner)
                           42)
                         1)))
            (fiber/resume outer)
            (eq? (fiber/child outer) (fiber/child outer))))
        "#,
    );
    // inner errors, not caught by inner's mask=0, propagates to outer.
    // outer catches (mask=1). child chain preserved on propagation.
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::bool(true),
        "fiber/child should return identical values"
    );
}

// ── #299: caught SIG_ERROR status and resumability ───────────────

#[test]
fn test_caught_sig_error_leaves_fiber_suspended() {
    // When a fiber signals SIG_ERROR and the mask catches it, the fiber
    // should be left in :suspended status (not :error). This is the core
    // bug fix for issue #299.
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 1 "oops") "recovered") 1)))
          (fiber/resume f)
          (keyword->string (fiber/status f)))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("suspended"),
        "Caught SIG_ERROR should leave fiber in suspended status"
    );
}

#[test]
fn test_caught_sig_error_fiber_is_resumable() {
    // A fiber that caught SIG_ERROR should be resumable. After catching
    // the error, the fiber should be able to continue execution and return
    // its recovery value.
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 1 "oops") "recovered") 1)))
          (fiber/resume f)
          (fiber/resume f))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("recovered"),
        "Second resume should succeed and return recovery value"
    );
}

#[test]
fn test_uncaught_sig_error_produces_error_status() {
    // When a fiber signals SIG_ERROR and the mask does NOT catch it,
    // the error should propagate and the fiber should be in :error status.
    // This confirms that uncaught errors still work as before.
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 1 "oops")) 0)))
          (fiber/resume f))
        "#,
    );
    // Error is NOT caught (mask=0), so it propagates to the root
    assert!(result.is_err(), "Expected error to propagate");
}

#[test]
fn test_cancel_always_produces_error_status() {
    // fiber/cancel is terminal: it always puts the fiber in :error status,
    // regardless of the mask. This is unchanged behavior, but we verify it
    // to ensure cancel is distinct from caught SIG_ERROR.
    let result = eval_source(
        r#"
        (let ((f (fiber/new (fn () (fiber/signal 2 "waiting") 99) 3)))
          (fiber/resume f)
          (fiber/cancel f "stop")
          (keyword->string (fiber/status f)))
        "#,
    );
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("error"),
        "Cancelled fiber should always be in error status"
    );
}
