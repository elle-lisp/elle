# Integration tests for arena/stats, arena/count, arena/allocs, and fiber heap isolation
#
# Migrated from tests/integration/arena.rs
# Tests that inspect bytecode stay in Rust (region instructions, etc.)

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ── arena/stats (struct form) ───────────────────────────────────────

# test_arena_stats_returns_struct
(let ((result (arena/stats)))
  (assert-true (struct? result) "arena/stats returns struct"))

# test_arena_stats_has_count_and_capacity
(let* ((stats (arena/stats))
       (count (get stats :count))
       (capacity (get stats :capacity)))
  (assert-true (>= count 0) "arena/stats count is non-negative")
  (assert-true (>= capacity 0) "arena/stats capacity is non-negative"))

# test_arena_stats_via_vm_query
(let ((result (vm/query "arena/stats" nil)))
  (assert-true (struct? result) "vm/query arena/stats returns struct"))

# ── arena/count (int form) ──────────────────────────────────────────

# test_arena_count_returns_int
(let ((result (arena/count)))
  (assert-true (int? result) "arena/count returns int")
  (assert-true (> result 0) "arena/count is positive after init"))

# test_arena_count_increases_with_allocation
(let* ((before (arena/count))
       (_ (list 1 2 3 4 5))
       (after (arena/count)))
  (assert-eq (> after before) true "arena count increases after allocation"))

# test_arena_count_overhead_is_zero
# arena/count operates directly on thread-local state (no SIG_QUERY)
(let* ((a (arena/count))
       (b (arena/count)))
  (assert-eq (- b a) 0 "arena/count has zero overhead"))

# ── arena/allocs (primitive) ────────────────────────────────────────

# test_arena_allocs_nil_thunk
# A no-op thunk should allocate 0 net objects
(let ((result (rest (arena/allocs (fn () nil)))))
  (assert-eq result 0 "nil thunk allocates 0 net objects"))

# test_arena_allocs_cons
(let ((result (rest (arena/allocs (fn () (cons 1 2))))))
  (assert-eq result 1 "cons allocates 1 object"))

# test_arena_allocs_preserves_result
(let ((result (first (arena/allocs (fn () (+ 40 2))))))
  (assert-eq result 42 "arena/allocs preserves return value"))

# test_arena_allocs_list
(let ((result (rest (arena/allocs (fn () (list 1 2 3 4 5))))))
  (assert-eq result 5 "list of 5 allocates 5 cons cells"))

# ── Fiber heap isolation ────────────────────────────────────────────

# test_child_fiber_has_own_arena
# Inside a child fiber, arena/count reports the child's FiberHeap,
# which starts empty. The child's count should be much smaller than
# the parent's (which includes all stdlib/primitive allocations).
(let* ((parent-count (arena/count))
       (f (fiber/new (fn () (arena/count)) 1))
       (child-count (fiber/resume f)))
  (assert-eq (< child-count parent-count) true
    "child fiber arena-count is less than parent's"))

# test_child_fiber_arena_starts_near_zero
# A child fiber's FiberHeap starts empty. The arena/count inside
# should be small (just overhead from the count query itself).
(let* ((f (fiber/new (fn () (arena/count)) 1))
       (child-count (fiber/resume f)))
  (assert-eq (< child-count 10) true
    "child fiber arena starts near zero"))

# test_parent_arena_restored_after_child
# After a child fiber completes, the parent's arena/count should
# continue from where it left off (not include child allocations).
(let* ((before (arena/count))
       (f (fiber/new (fn ()
             (list 1 2 3 4 5)
             (list 6 7 8 9 10))
           1))
       (_ (fiber/resume f))
       (after (arena/count)))
  # The difference should be small (just the fiber handle + overhead),
  # not include the 10 cons cells allocated in the child.
  (assert-eq (< (- after before) 10) true
    "child allocations don't inflate parent arena count"))

# test_child_fiber_allocations_tracked_separately
# Child fiber allocations go to its own FiberHeap.
# Verify by checking the count increases inside the child.
(let* ((f (fiber/new (fn ()
             (let* ((before (arena/count))
                    (_ (list 1 2 3 4 5))
                    (after (arena/count)))
               (- after before)))
           1)))
   (let ((allocs (fiber/resume f)))
     # list of 5 = 5 cons cells (arena/count has zero overhead)
     (assert-eq allocs 5
       "child sees exactly 5 allocations from list")))

# test_nested_fiber_heap_isolation
# Three levels: root → outer fiber → inner fiber.
# Each should have its own arena view.
(let* ((inner (fiber/new (fn () (arena/count)) 1))
       (outer (fiber/new (fn ()
                (let* ((outer-count (arena/count))
                       (inner-count (fiber/resume inner)))
                  (list outer-count inner-count)))
              1))
       (counts (fiber/resume outer)))
  # Both outer and inner counts should be small (near zero)
  (let* ((outer-c (first counts))
         (inner-c (first (rest counts))))
    (assert-eq (< outer-c 20) true "outer fiber arena is small")
    (assert-eq (< inner-c 10) true "inner fiber arena is small")))

# test_fiber_heap_survives_yield_resume
# Values allocated in a child fiber survive across yield/resume cycles
# because the FiberHeap persists on the Fiber struct.
(let* ((f (fiber/new (fn ()
             (emit 2 (cons 1 2))
             (cons 3 4))
           2))
       (first-val (fiber/resume f))
       (second-val (fiber/resume f)))
  (assert-eq (first first-val) 1 "first yield value first element")
  (assert-eq (first second-val) 3 "second yield value first element"))

# ── Leak detection: constant per-iter cost ──────────────────────────

# test_arena_eval_cost_is_constant
# Macro expansion cost per iteration must be stable across different N.
# If ArenaGuard is broken, per-iter cost would grow.
(let* ((measure (fn (n)
           (let* ((before (arena/count)))
             (letrec ((loop (fn (i)
                              (when (< i n)
                                (eval '(defn temp (x) (+ x 1)))
                                (loop (+ i 1))))))
               (loop 0))
             (/ (- (arena/count) before) n))))
       (p10 (measure 10))
       (p50 (measure 50)))
  (assert-eq (= p10 p50) true
    "per-iter allocation cost is constant"))

# ── Shared allocator / zero-copy fiber exchange ─────────────────────

# test_yielding_child_yields_string
# A yielding child allocates a string and yields it.
# The parent should be able to read the string after resume.
(let* ((f (fiber/new (fn () (emit 2 "hello")) 2))
       (result (fiber/resume f)))
  (assert-string-eq result "hello" "yielding child yields string"))

# test_non_yielding_child_no_overhead
# A non-yielding fiber (mask catches error only) should not get
# a shared allocator. The result is an immediate — no heap involved.
(let* ((f (fiber/new (fn () 42) 1))
       (result (fiber/resume f)))
  (assert-eq result 42 "non-yielding child returns immediate"))

# test_yield_resume_multiple_cycles
# Fiber yields twice (two resume cycles). Both values readable.
(let* ((f (fiber/new (fn ()
             (emit 2 "first")
             (emit 2 "second")
             "done")
           2))
       (v1 (fiber/resume f))
       (v2 (fiber/resume f))
       (v3 (fiber/resume f)))
  (assert-string-eq v1 "first" "first yield value")
  (assert-string-eq v2 "second" "second yield value")
  (assert-string-eq v3 "done" "final return value"))

# test_abc_chain_yield_through
# A→B→C: C yields a string, B catches and re-yields to A.
# Tests transitive shared_alloc propagation.
(let* ((c (fiber/new (fn () (emit 2 "from-c")) 2))
       (b (fiber/new (fn ()
             (let* ((val (fiber/resume c)))
               (emit 2 val)))
           2))
       (a-result (fiber/resume b)))
  (assert-string-eq a-result "from-c" "abc chain yield through"))

# test_root_child_yield
# Root resumes a yielding child. Child yields a string.
(let* ((f (fiber/new (fn () (emit 2 "from-child")) 2))
       (result (fiber/resume f)))
  (assert-string-eq result "from-child" "root child yield"))

# test_root_child_grandchild_yield
# Root→child→grandchild. Grandchild yields string,
# child yields it to root.
(let* ((gc (fiber/new (fn () (emit 2 "from-gc")) 2))
       (child (fiber/new (fn ()
                (let* ((val (fiber/resume gc)))
                  (emit 2 val)))
              2)))
  (let ((result (fiber/resume child)))
    (assert-string-eq result "from-gc" "root child grandchild yield")))

# test_child_death_value_survives
# Child yields a string then completes (dies).
# The yielded string should survive child death because it's
# in the shared allocator (owned by parent or child).
(let* ((f (fiber/new (fn ()
             (emit 2 "alive")
             "done")
           2))
       (yielded (fiber/resume f))
       (_ (fiber/resume f)))  # child dies here
  (assert-string-eq yielded "alive" "child death value survives"))

# test_multi_resume_yield_basic
# Multiple yields without letrec — tests shared alloc across resumes.
(let* ((f (fiber/new (fn ()
              (emit 2 0)
              (emit 2 1)
              (emit 2 2))
            2)))
  (let ((v1 (fiber/resume f))
        (v2 (fiber/resume f))
        (v3 (fiber/resume f)))
    (assert-eq v1 0 "multi resume yield basic: first")
    (assert-eq v2 1 "multi resume yield basic: second")
    (assert-eq v3 2 "multi resume yield basic: third")))

# test_multi_resume_yield_heap_values
# Yield heap-allocated values across multiple resumes.
# Tests that shared alloc keeps values alive for the parent.
(let* ((f (fiber/new (fn ()
              (emit 2 "hello")
              (emit 2 "world")
              (emit 2 "done"))
            2)))
  (let ((v1 (fiber/resume f))
        (v2 (fiber/resume f))
        (v3 (fiber/resume f)))
    (assert-string-eq v1 "hello" "multi resume heap: first")
    (assert-string-eq v2 "world" "multi resume heap: second")
    (assert-string-eq v3 "done" "multi resume heap: third")))

# test_multi_resume_yield_mixed_values
# Yield a mix of immediate and heap values across resumes.
(let* ((f (fiber/new (fn ()
              (emit 2 42)
              (emit 2 (list 1 2 3))
              (emit 2 "end"))
            2)))
  (let ((v1 (fiber/resume f))
        (v2 (fiber/resume f))
        (v3 (fiber/resume f)))
    (assert-eq v1 42 "multi resume mixed: first")
    (assert-eq (length v2) 3 "multi resume mixed: second is list")
    (assert-string-eq v3 "end" "multi resume mixed: third")))

# test_multiple_children_shared_allocs
# Parent resumes two different yielding children.
# Both yield strings. Both readable.
# Tests owned_shared Vec growth doesn't invalidate earlier pointers.
(let* ((f1 (fiber/new (fn () (emit 2 "from-f1")) 2))
       (f2 (fiber/new (fn () (emit 2 "from-f2")) 2))
       (v1 (fiber/resume f1))
       (v2 (fiber/resume f2)))
  (assert-string-eq v1 "from-f1" "multiple children: first")
  (assert-string-eq v2 "from-f2" "multiple children: second"))

# ── Lifecycle and edge cases ────────────────────────────────────────

# test_yield_immediate_no_shared_alloc_needed
# Yielding an immediate (int) requires no heap allocation.
# The shared alloc infrastructure should not interfere.
(let* ((f (fiber/new (fn () (emit 2 42)) 2))
       (result (fiber/resume f)))
  (assert-eq result 42 "yield immediate no shared alloc"))

# test_yield_list_parent_traverses
# Fiber yields a cons list. Parent traverses all elements.
# The list cells are heap-allocated — they go to shared alloc.
(let* ((f (fiber/new (fn () (emit 2 (list 10 20 30))) 2))
       (lst (fiber/resume f)))
  (assert-eq (first lst) 10 "yield list: first")
  (assert-eq (first (rest lst)) 20 "yield list: second")
  (assert-eq (first (rest (rest lst))) 30 "yield list: third"))

# test_yield_star_with_shared_alloc
# yield* delegates iteration. Values flow through shared alloc.
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
(assert-string-eq v1 "a" "yield star: first")
(assert-string-eq v2 "b" "yield star: second")

# test_error_in_child_with_shared_alloc
# Child fiber signals an error. The error value (a struct/tuple)
# is in shared space. Parent catches and reads the error message.
(let* ((f (fiber/new (fn () (error "test error")) 1))
       (_ (fiber/resume f))
       (val (fiber/value f)))
  (assert-true (not (nil? val)) "error in child with shared alloc"))

# test_cancel_child_with_shared_alloc
# Parent cancels a suspended child that has a shared allocator.
# Mask 3 catches both error (1) and yield (2) so cancel doesn't propagate.
(let* ((f (fiber/new (fn ()
              (emit 2 "yielded")
              "never-reached")
            3))
       (v1 (fiber/resume f)))      # child suspends
  (fiber/cancel f "cancelled")
  (let ((status (string (fiber/status f))))
    (assert-string-eq v1 "yielded" "cancel child: yielded value")
    (assert-string-eq status "error" "cancel child: status is error")))

# test_long_lived_coroutine_many_resumes
# Resume a coroutine 50 times, each time yielding a heap value (list).
# Exercises M2 — many shared allocs accumulate in owned_shared.
# All yielded values must be readable at the end.
(var gen (coro/new (fn ()
  (var i 0)
  (while (< i 50)
    (yield (list i (+ i 1)))
    (assign i (+ i 1))))))
(var results @[])
(while (not (coro/done? gen))
  (coro/resume gen nil)
  (when (not (coro/done? gen))
    (push results (coro/value gen))))
(assert-eq (length results) 50 "long lived coroutine: 50 yields")
(assert-eq (first (get results 0)) 0 "long lived coroutine: first yield")
(assert-eq (first (get results 49)) 49 "long lived coroutine: last yield")
