use crate::common::eval_source;
use elle::compiler::bytecode::disassemble_lines;
use elle::pipeline::compile;
use elle::SymbolTable;
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

// ── Region instruction emission ─────────────────────────────────────

fn bytecode_contains(source: &str, needle: &str) -> bool {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols).expect("compilation failed");
    let lines = disassemble_lines(&compiled.bytecode.instructions);
    lines.iter().any(|line| line.contains(needle))
}

fn count_in_bytecode(source: &str, needle: &str) -> usize {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols).expect("compilation failed");
    let lines = disassemble_lines(&compiled.bytecode.instructions);
    lines.iter().filter(|line| line.contains(needle)).count()
}

#[test]
fn test_let_no_region_when_result_is_var() {
    // Body returns a variable → result_is_safe returns false.
    // No scope allocation, no region instructions.
    assert!(!bytecode_contains("(let* ((x 1)) x)", "RegionEnter"));
    assert!(!bytecode_contains("(let* ((x 1)) x)", "RegionExit"));
}

#[test]
fn test_nested_let_regions_for_safe_body() {
    // Inner let: body is (+ x y) — intrinsic call, result is immediate.
    // No captures, pure body → inner let qualifies for scope allocation.
    // Outer let: body is the inner let — result_is_safe returns false
    // for Let nodes (wildcard), so outer let does NOT scope-allocate.
    let source = "(let* ((x 1)) (let* ((y 2)) (+ x y)))";
    let enters = count_in_bytecode(source, "RegionEnter");
    let exits = count_in_bytecode(source, "RegionExit");
    assert_eq!(enters, 1, "inner let should emit RegionEnter");
    assert_eq!(exits, 1, "inner let should emit RegionExit");
}

#[test]
fn test_block_region_for_literal_body() {
    // Block body is a literal → result is immediate, no suspension,
    // no breaks, no outward set. Block qualifies for scope allocation.
    assert!(bytecode_contains("(block :done 42)", "RegionEnter"));
    assert!(bytecode_contains("(block :done 42)", "RegionExit"));
}

#[test]
fn test_fn_body_no_region_instructions() {
    // Function bodies should NOT emit region instructions (per plan)
    // The function itself is a closure in the constant pool, so we check
    // that the top-level bytecode does NOT contain RegionEnter
    // (the fn expression compiles to MakeClosure, not region instructions)
    let source = "(fn (x) (+ x 1))";
    assert!(
        !bytecode_contains(source, "RegionEnter"),
        "fn body should not emit RegionEnter at top level"
    );
}

#[test]
fn test_scoped_execution_results_unchanged() {
    // Verify execution results are unchanged (no region instructions emitted,
    // so behavior is identical to pre-Package 5)
    let result = eval_source("(let* ((x 10) (y 20)) (+ x y))").unwrap();
    assert_eq!(result, Value::int(30));

    let result = eval_source("(block :done (let* ((x 5)) (+ x x)))").unwrap();
    assert_eq!(result, Value::int(10));

    let result = eval_source(
        "(let* ((a 1))
           (let* ((b 2))
             (let* ((c 3))
               (+ a (+ b c)))))",
    )
    .unwrap();
    assert_eq!(result, Value::int(6));
}

// ── Break compensating exits ────────────────────────────────────────

#[test]
fn test_break_no_compensating_exits_conservative() {
    // Under conservative escape analysis, no region instructions are emitted,
    // so break has no compensating exits to emit either.
    let source = "(block :done (let* ((x 1)) (break :done 42)))";
    let exits = count_in_bytecode(source, "RegionExit");
    assert_eq!(
        exits, 0,
        "conservative: no RegionExit emitted, so no compensating exits"
    );
}

#[test]
fn test_break_from_nested_let_correct_result() {
    // Verify break from inside a nested let actually works correctly
    let result = eval_source(
        "(block :done
           (let* ((x 10))
             (let* ((y 20))
               (break :done (+ x y)))))",
    )
    .unwrap();
    assert_eq!(result, Value::int(30));
}

#[test]
fn test_break_from_nested_let_in_child_fiber() {
    // Same test but inside a child fiber — exercises real scope marks
    let result = eval_source(
        "(let* ((f (fiber/new
                     (fn ()
                        (block :done
                          (let* ((x 10))
                            (let* ((y 20))
                              (break :done (+ x y))))))
                      1)))
           (fiber/resume f))",
    )
    .unwrap();
    assert_eq!(result, Value::int(30));
}

// ── Shared allocator / zero-copy fiber exchange ─────────────────────

#[test]
fn test_yielding_child_yields_string() {
    // A yielding child allocates a string and yields it.
    // The parent should be able to read the string after resume.
    let result = eval_source(
        "(let* ((f (fiber/new (fn () (fiber/signal 2 \"hello\")) 2)))
           (fiber/resume f))",
    )
    .unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "hello");
}

#[test]
fn test_non_yielding_child_no_overhead() {
    // A non-yielding fiber (mask catches error only) should not get
    // a shared allocator. The result is an immediate — no heap involved.
    let result = eval_source(
        "(let* ((f (fiber/new (fn () 42) 1)))
           (fiber/resume f))",
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_yield_resume_multiple_cycles() {
    // Fiber yields twice (two resume cycles). Both values readable.
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                      (fiber/signal 2 \"first\")
                      (fiber/signal 2 \"second\")
                      \"done\")
                    2))
                (v1 (fiber/resume f))
                (v2 (fiber/resume f))
                (v3 (fiber/resume f)))
           (list v1 v2 v3))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0].with_string(|s| s.to_string()).unwrap(), "first");
    assert_eq!(vec[1].with_string(|s| s.to_string()).unwrap(), "second");
    assert_eq!(vec[2].with_string(|s| s.to_string()).unwrap(), "done");
}

#[test]
fn test_abc_chain_yield_through() {
    // A→B→C: C yields a string, B catches and re-yields to A.
    // Tests transitive shared_alloc propagation (N1).
    let result = eval_source(
        "(let* ((c (fiber/new (fn () (fiber/signal 2 \"from-c\")) 2))
                (b (fiber/new (fn ()
                      (let* ((val (fiber/resume c)))
                        (fiber/signal 2 val)))
                    2))
                (a-result (fiber/resume b)))
           a-result)",
    )
    .unwrap();
    assert!(result.is_string());
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "from-c");
}

#[test]
fn test_root_child_yield() {
    // Root resumes a yielding child. Child yields a string.
    // (Root→child: child creates shared alloc on its own heap.)
    let result = eval_source(
        "(let* ((f (fiber/new (fn () (fiber/signal 2 \"from-child\")) 2)))
           (fiber/resume f))",
    )
    .unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "from-child");
}

#[test]
fn test_root_child_grandchild_yield() {
    // Root→child→grandchild. Grandchild yields string,
    // child yields it to root.
    let result = eval_source(
        "(let* ((gc (fiber/new (fn () (fiber/signal 2 \"from-gc\")) 2))
                (child (fiber/new (fn ()
                         (let* ((val (fiber/resume gc)))
                           (fiber/signal 2 val)))
                       2)))
           (fiber/resume child))",
    )
    .unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "from-gc");
}

#[test]
fn test_child_death_value_survives() {
    // Child yields a string then completes (dies).
    // The yielded string should survive child death because it's
    // in the shared allocator (owned by parent or child).
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                      (fiber/signal 2 \"alive\")
                      \"done\")
                    2))
                (yielded (fiber/resume f))
                (_ (fiber/resume f)))  # child dies here
           yielded)", // read the previously yielded value
    )
    .unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "alive");
}

#[test]
fn test_multi_resume_yield_basic() {
    // Multiple yields without letrec — tests shared alloc across resumes.
    // (letrec + yield has a known pre-existing bug, see issue)
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                       (fiber/signal 2 0)
                       (fiber/signal 2 1)
                       (fiber/signal 2 2))
                     2)))
           (list (fiber/resume f) (fiber/resume f) (fiber/resume f)))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(0));
    assert_eq!(vec[1], Value::int(1));
    assert_eq!(vec[2], Value::int(2));
}

#[test]
fn test_multi_resume_yield_heap_values() {
    // Yield heap-allocated values across multiple resumes.
    // Tests that shared alloc keeps values alive for the parent.
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                       (fiber/signal 2 \"hello\")
                       (fiber/signal 2 \"world\")
                       (fiber/signal 2 \"done\"))
                     2)))
           (list (fiber/resume f) (fiber/resume f) (fiber/resume f)))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0].with_string(|s| s.to_string()).unwrap(), "hello");
    assert_eq!(vec[1].with_string(|s| s.to_string()).unwrap(), "world");
    assert_eq!(vec[2].with_string(|s| s.to_string()).unwrap(), "done");
}

#[test]
fn test_multi_resume_yield_mixed_values() {
    // Yield a mix of immediate and heap values across resumes.
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                       (fiber/signal 2 42)
                       (fiber/signal 2 (list 1 2 3))
                       (fiber/signal 2 \"end\"))
                     2)))
           (list (fiber/resume f) (fiber/resume f) (fiber/resume f)))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(42));
    let inner = vec[1].list_to_vec().unwrap();
    assert_eq!(inner, vec![Value::int(1), Value::int(2), Value::int(3)]);
    assert_eq!(vec[2].with_string(|s| s.to_string()).unwrap(), "end");
}

#[test]
fn test_multiple_children_shared_allocs() {
    // Parent resumes two different yielding children.
    // Both yield strings. Both readable.
    // Tests owned_shared Vec growth doesn't invalidate earlier pointers.
    let result = eval_source(
        "(let* ((f1 (fiber/new (fn () (fiber/signal 2 \"from-f1\")) 2))
                (f2 (fiber/new (fn () (fiber/signal 2 \"from-f2\")) 2))
                (v1 (fiber/resume f1))
                (v2 (fiber/resume f2)))
           (list v1 v2))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0].with_string(|s| s.to_string()).unwrap(), "from-f1");
    assert_eq!(vec[1].with_string(|s| s.to_string()).unwrap(), "from-f2");
}

// ── Lifecycle and edge cases ────────────────────────────────────────

#[test]
fn test_yield_immediate_no_shared_alloc_needed() {
    // Yielding an immediate (int) requires no heap allocation.
    // The shared alloc infrastructure should not interfere.
    let result = eval_source(
        "(let* ((f (fiber/new (fn () (fiber/signal 2 42)) 2)))
           (fiber/resume f))",
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_yield_list_parent_traverses() {
    // Fiber yields a cons list. Parent traverses all elements.
    // The list cells are heap-allocated — they go to shared alloc.
    let result = eval_source(
        "(let* ((f (fiber/new (fn () (fiber/signal 2 (list 10 20 30))) 2)))
           (let* ((lst (fiber/resume f)))
             (list (first lst) (first (rest lst)) (first (rest (rest lst))))))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(10));
    assert_eq!(vec[1], Value::int(20));
    assert_eq!(vec[2], Value::int(30));
}

#[test]
fn test_yield_star_with_shared_alloc() {
    // yield* delegates iteration. Values flow through shared alloc.
    let result = eval_source(
        r#"
        (def sub (coro/new (fn ()
          (yield "a")
          (yield "b")
          :done)))
        (def main (coro/new (fn ()
          (yield* sub))))
        (coro/resume main nil)
        (def v1 (coro/value main))
        (coro/resume main nil)
        (def v2 (coro/value main))
        (list v1 v2)
        "#,
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0].with_string(|s| s.to_string()).unwrap(), "a");
    assert_eq!(vec[1].with_string(|s| s.to_string()).unwrap(), "b");
}

#[test]
fn test_error_in_child_with_shared_alloc() {
    // Child fiber raises an error. The error value (a struct/tuple)
    // is in shared space. Parent catches and reads the error message.
    let result = eval_source(
        "(let* ((f (fiber/new (fn () (error \"test error\")) 1)))
           (fiber/resume f)
           (let* ((val (fiber/value f)))
             (if (nil? val) \"no-value\" val)))",
    )
    .unwrap();
    // The error value should be readable by the parent.
    // fiber/value returns the signal value (error tuple).
    assert!(!result.is_nil());
}

#[test]
fn test_cancel_child_with_shared_alloc() {
    // Parent cancels a suspended child that has a shared allocator.
    // Mask 3 catches both error (1) and yield (2) so cancel doesn't propagate.
    let result = eval_source(
        "(let* ((f (fiber/new (fn ()
                       (fiber/signal 2 \"yielded\")
                       \"never-reached\")
                     3))
                (v1 (fiber/resume f)))      # child suspends
           (fiber/cancel f \"cancelled\")
           (list v1 (keyword->string (fiber/status f))))",
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0].with_string(|s| s.to_string()).unwrap(), "yielded");
    assert_eq!(vec[1].with_string(|s| s.to_string()).unwrap(), "error");
}

#[test]
fn test_long_lived_coroutine_many_resumes() {
    // Resume a coroutine 50 times, each time yielding a heap value (list).
    // Exercises M2 — many shared allocs accumulate in owned_shared.
    // All yielded values must be readable at the end.
    let result = eval_source(
        r#"
        (var gen (coro/new (fn ()
          (var i 0)
          (while (< i 50)
            (yield (list i (+ i 1)))
            (set i (+ i 1))))))
        (var results @[])
        (while (not (coro/done? gen))
          (coro/resume gen nil)
          (when (not (coro/done? gen))
            (push results (coro/value gen))))
        (list (length results)
              (first (get results 0))
              (first (get results 49)))
        "#,
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(50));
    assert_eq!(vec[1], Value::int(0));
    assert_eq!(vec[2], Value::int(49));
}
