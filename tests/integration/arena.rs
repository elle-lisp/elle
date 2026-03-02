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

// ── Fiber heap isolation ────────────────────────────────────────────

#[test]
fn test_child_fiber_has_own_arena() {
    // Inside a child fiber, arena-count reports the child's FiberHeap,
    // which starts empty. The child's count should be much smaller than
    // the parent's (which includes all stdlib/primitive allocations).
    let result = eval_source(
        "(let* ((parent-count (arena-count))
                (f (fiber/new (fn () (arena-count)) 1))
                (child-count (fiber/resume f)))
           (< child-count parent-count))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "child fiber arena-count should be less than parent's"
    );
}

#[test]
fn test_child_fiber_arena_starts_near_zero() {
    // A child fiber's FiberHeap starts empty. The arena-count inside
    // should be small (just overhead from the count query itself).
    let result = eval_source(
        "(let* ((f (fiber/new (fn () (arena-count)) 1))
                (child-count (fiber/resume f)))
           (< child-count 10))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "child fiber arena should start near zero"
    );
}

#[test]
fn test_parent_arena_restored_after_child() {
    // After a child fiber completes, the parent's arena-count should
    // continue from where it left off (not include child allocations).
    let result = eval_source(
        "(let* ((before (arena-count))
                (f (fiber/new (fn ()
                      (list 1 2 3 4 5)
                      (list 6 7 8 9 10))
                    1))
                (_ (fiber/resume f))
                (after (arena-count)))
           # The difference should be small (just the fiber handle + overhead),
           # not include the 10 cons cells allocated in the child.
           (< (- after before) 10))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "child allocations should not inflate parent arena count"
    );
}

#[test]
fn test_child_fiber_allocations_tracked_separately() {
    // Child fiber allocations go to its own FiberHeap.
    // Verify by checking the count increases inside the child.
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                      (let* ((before (arena-count))
                             (_ (list 1 2 3 4 5))
                             (after (arena-count)))
                        (- after before)))
                    1)))
           (fiber/resume f))",
    )
    .unwrap();
    // list of 5 = 5 cons cells, plus 1 overhead for the arena-count query
    let allocs = result.as_int().unwrap();
    assert!(
        (5..=7).contains(&allocs),
        "child should see 5-7 allocations from list, got {}",
        allocs
    );
}

#[test]
fn test_nested_fiber_heap_isolation() {
    // Three levels: root → outer fiber → inner fiber.
    // Each should have its own arena view.
    let result = eval_source(
        "(let* ((inner (fiber/new (fn () (arena-count)) 1))
                (outer (fiber/new (fn ()
                         (let* ((outer-count (arena-count))
                                (inner-count (fiber/resume inner)))
                           (list outer-count inner-count)))
                       1))
                (counts (fiber/resume outer)))
           # Both outer and inner counts should be small (near zero)
           (let* ((outer-c (first counts))
                  (inner-c (first (rest counts))))
             (list (< outer-c 20) (< inner-c 10))))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0], Value::TRUE, "outer fiber arena should be small");
    assert_eq!(vec[1], Value::TRUE, "inner fiber arena should be small");
}

#[test]
fn test_fiber_heap_survives_yield_resume() {
    // Values allocated in a child fiber survive across yield/resume cycles
    // because the FiberHeap persists on the Fiber struct.
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                      (fiber/signal 2 (cons 1 2))
                      (cons 3 4))
                    2))
                (first-val (fiber/resume f))
                (second-val (fiber/resume f)))
           (list (first first-val) (first second-val)))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(3));
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
