use crate::common::eval_source;
use elle::Value;

// ── vm/arena (struct form) ──────────────────────────────────────────

#[test]
fn test_vm_arena_returns_struct() {
    let result = eval_source("(vm/arena)").unwrap();
    assert!(
        result.as_struct().is_some(),
        "vm/arena should return a struct"
    );
}

#[test]
fn test_vm_arena_has_count_and_capacity() {
    let result = eval_source(
        "(let* ((stats (vm/arena)))
           (list (get stats :count) (get stats :capacity)))",
    )
    .unwrap();
    let first = result.as_cons().unwrap().first;
    let rest_cons = result.as_cons().unwrap().rest.as_cons().unwrap();
    let second = rest_cons.first;
    assert!(first.as_int().unwrap() >= 0, "count should be non-negative");
    assert!(
        second.as_int().unwrap() >= 0,
        "capacity should be non-negative"
    );
}

#[test]
fn test_vm_arena_via_vm_query() {
    let result = eval_source("(vm/query :arena nil)").unwrap();
    assert!(
        result.as_struct().is_some(),
        "vm/query :arena should return a struct"
    );
}

// ── arena-count (int form) ──────────────────────────────────────────

#[test]
fn test_arena_count_returns_int() {
    let result = eval_source("(arena-count)").unwrap();
    assert!(
        result.as_int().is_some(),
        "arena-count should return an int"
    );
    assert!(
        result.as_int().unwrap() > 0,
        "arena-count should be positive after init"
    );
}

#[test]
fn test_arena_count_increases_with_allocation() {
    let result = eval_source(
        "(let* ((before (arena-count))
                (_ (list 1 2 3 4 5))
                (after (arena-count)))
           (> after before))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "arena count should increase after allocation"
    );
}

#[test]
fn test_arena_count_overhead_is_one() {
    // Each arena-count call allocates exactly 1 cons (SIG_QUERY message)
    let result = eval_source(
        "(let* ((a (arena-count))
                (b (arena-count)))
           (- b a))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::int(1),
        "arena-count overhead should be exactly 1"
    );
}

// ── arena/allocs (stdlib helper) ────────────────────────────────────

#[test]
fn test_arena_allocs_nil_thunk() {
    // A no-op thunk should allocate 0 net objects
    let result = eval_source("(first (rest (arena/allocs (fn () nil))))").unwrap();
    assert_eq!(
        result,
        Value::int(0),
        "nil thunk should allocate 0 net objects"
    );
}

#[test]
fn test_arena_allocs_cons() {
    let result = eval_source("(first (rest (arena/allocs (fn () (cons 1 2)))))").unwrap();
    assert_eq!(result, Value::int(1), "cons should allocate 1 object");
}

#[test]
fn test_arena_allocs_preserves_result() {
    let result = eval_source("(first (arena/allocs (fn () (+ 40 2))))").unwrap();
    assert_eq!(
        result,
        Value::int(42),
        "arena/allocs should preserve return value"
    );
}

#[test]
fn test_arena_allocs_list() {
    let result = eval_source("(first (rest (arena/allocs (fn () (list 1 2 3 4 5)))))").unwrap();
    assert_eq!(
        result,
        Value::int(5),
        "list of 5 should allocate 5 cons cells"
    );
}

// ── Leak detection: constant per-iter cost ──────────────────────────

#[test]
fn test_arena_eval_cost_is_constant() {
    // Macro expansion cost per iteration must be stable across different N.
    // If ArenaGuard is broken, per-iter cost would grow.
    let result = eval_source(
        "(let* ((measure (fn (n)
                  (let* ((before (arena-count)))
                    (letrec ((loop (fn (i)
                                     (when (< i n)
                                       (eval '(defn temp (x) (+ x 1)))
                                       (loop (+ i 1))))))
                      (loop 0))
                    (/ (- (arena-count) before 1) n))))
                (p10 (measure 10))
                (p50 (measure 50)))
           (= p10 p50))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "per-iter allocation cost should be constant"
    );
}
