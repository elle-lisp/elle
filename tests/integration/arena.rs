use crate::common::eval_source;

// ── Object limit tests ──────────────────────────────────────────────

#[test]
fn test_object_limit_catchable_error() {
    // try/catch wraps the body in a child fiber (with its own FiberHeap),
    // so the limit must be set INSIDE the try body to target the fiber's heap.
    let result = eval_source(
        "(try
          (begin
            (arena/set-object-limit (+ (arena/count) 20))
            (var i 0)
            (while (< i 1000)
              (cons i nil)
              (set i (+ i 1)))
            \"no-error\")
          (catch e
            (get e :error)))",
    )
    .unwrap();
    assert_eq!(
        result.as_keyword_name(),
        Some("allocation-error"),
        "expected :allocation-error, got {:?}",
        result
    );
}

#[test]
fn test_set_object_limit_returns_previous() {
    // Use large limits (well above current count) to avoid triggering the limit.
    // First set returns nil (no previous limit), second returns the first limit.
    let result = eval_source(
        "(let* ([base (arena/count)]
               [big1 (+ base 10000)]
               [big2 (+ base 20000)])
          (arena/set-object-limit big1 :global)
          (let ([prev (arena/set-object-limit big2 :global)])
            (arena/set-object-limit nil :global)
            (- prev base)))",
    )
    .unwrap();
    assert_eq!(result.as_int(), Some(10000));
}

#[test]
fn test_object_limit_reads_back() {
    let result = eval_source(
        "(let* ([base (arena/count)]
               [lim (+ base 50000)])
          (arena/set-object-limit lim :global)
          (let ([got (arena/object-limit :global)])
            (arena/set-object-limit nil :global)
            (= got lim)))",
    )
    .unwrap();
    assert_eq!(result, elle::Value::TRUE);
}

#[test]
fn test_arena_stats_includes_object_limit() {
    let result = eval_source(
        "(let* ([base (arena/count)]
               [lim (+ base 99999)])
          (arena/set-object-limit lim :global)
          (let ([stats (arena/stats)])
            (arena/set-object-limit nil :global)
            (= (get stats :object-limit) lim)))",
    )
    .unwrap();
    assert_eq!(result, elle::Value::TRUE);
}

#[test]
fn test_arena_stats_includes_bytes() {
    let result = eval_source("(get (arena/stats) :bytes)").unwrap();
    // bytes should be a positive integer (count * 128)
    assert!(result.as_int().unwrap() > 0, "expected positive bytes");
}

#[test]
fn test_nil_limit_means_unlimited() {
    // Setting nil should clear the limit. Use a large limit to avoid triggering it.
    let result = eval_source(
        "(let* ([base (arena/count)]
               [lim (+ base 99999)])
          (arena/set-object-limit lim :global)
          (arena/set-object-limit nil :global)
          (arena/object-limit :global))",
    )
    .unwrap();
    assert!(
        result.is_nil(),
        "expected nil (unlimited), got {:?}",
        result
    );
}

#[test]
fn test_arena_bytes_returns_int() {
    let result = eval_source("(arena/bytes :global)").unwrap();
    assert!(result.as_int().is_some(), "expected integer");
    assert!(result.as_int().unwrap() > 0, "expected positive bytes");
}
