use crate::common::eval_source;

// ── Checkpoint/reset tests ──────────────────────────────────────────

#[test]
fn test_checkpoint_reset_reclaims_objects() {
    // arena/count has 1 object overhead (SIG_QUERY cons), so after reset
    // the count will be mark + 1 when we measure it.
    let result = eval_source(
        "(let ((mark (arena/checkpoint)))
           (cons 1 2)
           (cons 3 4)
           (cons 5 6)
           (list 7 8 9)
           (arena/reset mark)
           (- (arena/count) mark))",
    )
    .unwrap();
    // Exactly 1: the SIG_QUERY cons from arena/count itself
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_reset_with_invalid_mark_errors() {
    // arena/checkpoint reads root arena; adding 999 guarantees mark > current.
    let result = eval_source(
        "(try
           (arena/reset (+ (arena/checkpoint) 999))
           (catch e (get e :error)))",
    )
    .unwrap();
    assert_eq!(
        result.as_keyword_name(),
        Some("value-error"),
        "expected :value-error, got {:?}",
        result
    );
}

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

// ── Scope bump reclamation tests ────────────────────────────────────

#[test]
fn test_scope_bump_reclaims_memory() {
    // Run in a child fiber so FiberHeap is active.
    // Allocate many objects in a scoped let, verify arena/bytes drops
    // after scope exit. The let must qualify for scope allocation
    // (result must be immediate).
    let result = eval_source(
        "(fiber/resume (fiber/new (fn []
           (var bytes-before (arena/bytes))
           (let* ([x @[1 2 3 4 5 6 7 8 9 10]] [n (length x)])
             n)
           (var bytes-after (arena/bytes))
           (< bytes-after (+ bytes-before 1000))) 1))",
    )
    .unwrap();
    assert_eq!(
        result,
        elle::Value::TRUE,
        "scope bump should reclaim memory: bytes-after should be close to bytes-before"
    );
}

// ── arena/allocs primitive tests ────────────────────────────────────

#[test]
fn test_arena_allocs_primitive_cons() {
    let result = eval_source("(rest (arena/allocs (fn [] (cons 1 2))))").unwrap();
    assert_eq!(result.as_int(), Some(1), "cons allocates 1 object");
}

#[test]
fn test_arena_allocs_zero_for_immediate() {
    let result = eval_source("(rest (arena/allocs (fn [] 42)))").unwrap();
    assert_eq!(result.as_int(), Some(0), "immediate allocates 0 objects");
}

#[test]
fn test_arena_allocs_preserves_result() {
    let result = eval_source("(first (arena/allocs (fn [] (+ 40 2))))").unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_arena_allocs_list() {
    let result = eval_source("(rest (arena/allocs (fn [] (list 1 2 3 4 5))))").unwrap();
    assert_eq!(result.as_int(), Some(5), "list of 5 = 5 cons cells");
}

// ── arena/peak tests ────────────────────────────────────────────────

#[test]
fn test_arena_peak_returns_int() {
    let result = eval_source("(arena/peak :global)").unwrap();
    assert!(result.as_int().is_some(), "arena/peak returns int");
}

#[test]
fn test_arena_peak_gte_count() {
    // arena/count allocates a SIG_QUERY cons, so read count first to
    // ensure peak is updated, then check peak >= that count value.
    let result = eval_source(
        "(let ((c (arena/count)))
           (>= (arena/peak :global) c))",
    )
    .unwrap();
    assert_eq!(result, elle::Value::TRUE, "peak >= count");
}

#[test]
fn test_arena_reset_peak_returns_previous() {
    let result = eval_source(
        "(begin
           (arena/reset-peak :global)
           (cons 1 2)
           (cons 3 4)
           (let ((p (arena/peak :global)))
             (let ((prev (arena/reset-peak :global)))
               (= prev p))))",
    )
    .unwrap();
    assert_eq!(result, elle::Value::TRUE);
}

// ── arena/stats includes :peak ──────────────────────────────────────

#[test]
fn test_arena_stats_includes_peak() {
    let result = eval_source("(int? (get (arena/stats) :peak))").unwrap();
    assert_eq!(result, elle::Value::TRUE, "arena/stats has :peak field");
}

// ── arena/fiber-stats tests ─────────────────────────────────────────

#[test]
fn test_arena_fiber_stats_suspended() {
    let result = eval_source(
        "(let* ([f (fiber/new (fn [] (fiber/signal 2 42)) 2)]
                [_ (fiber/resume f)])
           (let ((stats (arena/fiber-stats f)))
             (and (struct? stats)
                  (int? (get stats :count))
                  (int? (get stats :bytes))
                  (int? (get stats :peak))
                  (int? (get stats :scope-enters))
                  (int? (get stats :dtors-run)))))",
    )
    .unwrap();
    assert_eq!(
        result,
        elle::Value::TRUE,
        "fiber-stats returns struct with expected fields"
    );
}

#[test]
fn test_arena_fiber_stats_dead() {
    let result = eval_source(
        "(let* ([f (fiber/new (fn [] 42) 1)]
                [_ (fiber/resume f)])
           (struct? (arena/fiber-stats f)))",
    )
    .unwrap();
    assert_eq!(
        result,
        elle::Value::TRUE,
        "fiber-stats works on dead fibers"
    );
}

#[test]
fn test_arena_fiber_stats_type_error() {
    let result = eval_source("(try (arena/fiber-stats 42) (catch e (get e :error)))").unwrap();
    assert_eq!(result.as_keyword_name(), Some("type-error"));
}
