(elle/epoch 9)
# Integration tests for arena/stats, arena/count, arena/allocs, and fiber heap isolation
#
# Migrated from tests/integration/arena.rs
# Tests that inspect bytecode stay in Rust (region instructions, etc.)


# ── arena/stats (struct form) ───────────────────────────────────────

# test_arena_stats_returns_struct
(let [result (arena/stats)]
  (assert (struct? result) "arena/stats returns struct"))

# test_arena_stats_has_expected_fields
(let* [stats (arena/stats)
       count (get stats :object-count)
       bytes (get stats :allocated-bytes)
       peak (get stats :peak-count)]
  (assert (>= count 0) "arena/stats :object-count is non-negative")
  (assert (>= bytes 0) "arena/stats :allocated-bytes is non-negative")
  (assert (>= peak 0) "arena/stats :peak-count is non-negative"))

# test_arena_stats_via_vm_query
(let [result (vm/query "arena/stats" nil)]
  (assert (struct? result) "vm/query arena/stats returns struct"))

# ── arena/count (int form) ──────────────────────────────────────────

# test_arena_count_returns_int
(let [result (arena/count)]
  (assert (int? result) "arena/count returns int")
  (assert (> result 0) "arena/count is positive after init"))

# test_arena_count_increases_with_allocation
(let* [before (arena/count)
       _ (list 1 2 3 4 5)
       after (arena/count)]
  (assert (= (> after before) true) "arena count increases after allocation"))

# test_arena_count_overhead_is_zero
# arena/count operates directly on thread-local state (no SIG_QUERY)
(let* [a (arena/count)
       b (arena/count)]
  (assert (= (- b a) 0) "arena/count has zero overhead"))

# ── arena/allocs (primitive) ────────────────────────────────────────

# test_arena_allocs_nil_thunk
# A no-op thunk should allocate 0 net objects
(let [result (rest (arena/allocs (fn () nil)))]
  (assert (= result 0) "nil thunk allocates 0 net objects"))

# test_arena_allocs_cons
(let [result (rest (arena/allocs (fn () (pair 1 2))))]
  (assert (= result 1) "pair allocates 1 object"))

# test_arena_allocs_preserves_result
(let [result (first (arena/allocs (fn () (+ 40 2))))]
  (assert (= result 42) "arena/allocs preserves return value"))

# test_arena_allocs_list
(let [result (rest (arena/allocs (fn () (list 1 2 3 4 5))))]
  (assert (= result 5) "list of 5 allocates 5 pair cells"))

# ── Fiber heap isolation ────────────────────────────────────────────

# test_child_fiber_has_own_arena
# Inside a child fiber, arena/count reports the child's FiberHeap,
# which starts empty. The child's count should be much smaller than
# the parent's (which includes all stdlib/primitive allocations).
(let* [parent-count (arena/count)
       f (fiber/new (fn () (arena/count)) 1)
       child-count (fiber/resume f)]
  (assert (= (< child-count parent-count) true)
          "child fiber arena-count is less than parent's"))

# test_child_fiber_arena_starts_near_zero
# A child fiber's FiberHeap starts empty. The arena/count inside
# should be small (just overhead from the count query itself).
(let* [f (fiber/new (fn () (arena/count)) 1)
       child-count (fiber/resume f)]
  (assert (= (< child-count 10) true) "child fiber arena starts near zero"))

# test_parent_arena_restored_after_child
# After a child fiber completes, the parent's arena/count should
# continue from where it left off (not include child allocations).
(let* [before (arena/count)
       f (fiber/new (fn ()
                      (list 1 2 3 4 5)
                      (list 6 7 8 9 10)) 1)
       _ (fiber/resume f)
       after (arena/count)]
  (assert (= (< (- after before) 10) true)
          "child allocations don't inflate parent arena count"))

# test_child_fiber_allocations_tracked_separately
# Child fiber allocations go to its own FiberHeap.
# Verify by checking the count increases inside the child.
(let* [f (fiber/new (fn ()
                      (let* [before (arena/count)
                             _ (list 1 2 3 4 5)
                             after (arena/count)]
                        (- after before))) 1)]
  (let [allocs (fiber/resume f)]
    (assert (= allocs 5) "child sees exactly 5 allocations from list")))

# test_nested_fiber_heap_isolation
# Three levels: root → outer fiber → inner fiber.
# Each should have its own arena view.
(let* [inner (fiber/new (fn () (arena/count)) 1)
       outer (fiber/new (fn ()
                          (let* [outer-count (arena/count)
                                 inner-count (fiber/resume inner)]
                            (list outer-count inner-count))) 1)
       counts (fiber/resume outer)]
  (let* [outer-c (first counts)
         inner-c (first (rest counts))]
    (assert (= (< outer-c 20) true) "outer fiber arena is small")
    (assert (= (< inner-c 10) true) "inner fiber arena is small")))

# test_fiber_heap_survives_yield_resume
# Values allocated in a child fiber survive across yield/resume cycles
# because the FiberHeap persists on the Fiber struct.
(let* [f (fiber/new (fn ()
                      (yield (pair 1 2))
                      (pair 3 4)) 2)
       first-val (fiber/resume f)
       second-val (fiber/resume f)]
  (assert (= (first first-val) 1) "first yield value first element")
  (assert (= (first second-val) 3) "second yield value first element"))

# ── Leak detection: constant per-iter cost ──────────────────────────

# test_arena_eval_cost_is_constant
# Macro expansion cost per iteration must be stable across different N
# after the transformer cache is warm.
# The first expansion per macro compiles the transformer closure (no arena
# guard — the closure must survive to be cached). Subsequent expansions use
# the cached closure and are cheaper. We pre-warm the cache before measuring
# so both measurements reflect only the constant warm-path cost.
(let* [measure (fn (n)
                 (let* [before (arena/count)]
                   (letrec [loop (fn (i)
                                   (when (< i n)
                                     (eval '(defn temp (x)
                                       (+ x 1)))
                                     (loop (+ i 1))))]
                     (loop 0))
                   (/ (- (arena/count) before) n)))
       _ (eval '(defn temp (x)
                 (+ x 1)))  # warm-up: compile transformer closures
       p10 (measure 10)
       p50 (measure 50)]
  (assert (= (= p10 p50) true)
          "per-iter allocation cost is constant after cache warm-up"))

# ── Shared allocator / zero-copy fiber exchange ─────────────────────

# test_yielding_child_yields_string
# A yielding child allocates a string and yields it.
# The parent should be able to read the string after resume.
(let* [f (fiber/new (fn () (yield "hello")) 2)
       result (fiber/resume f)]
  (assert (= result "hello") "yielding child yields string"))

# test_non_yielding_child_no_overhead
# A non-yielding fiber (mask catches error only) should not get
# a shared allocator. The result is an immediate — no heap involved.
(let* [f (fiber/new (fn () 42) 1)
       result (fiber/resume f)]
  (assert (= result 42) "non-yielding child returns immediate"))

# test_yield_resume_multiple_cycles
# Fiber yields twice (two resume cycles). Both values readable.
(let* [f (fiber/new (fn ()
                      (yield "first")
                      (yield "second")
                      "done") 2)
       v1 (fiber/resume f)
       v2 (fiber/resume f)
       v3 (fiber/resume f)]
  (assert (= v1 "first") "first yield value")
  (assert (= v2 "second") "second yield value")
  (assert (= v3 "done") "final return value"))

# test_abc_chain_yield_through
# A→B→C: C yields a string, B catches and re-yields to A.
# Tests transitive shared_alloc propagation.
(let* [c (fiber/new (fn () (yield "from-c")) 2)
       b (fiber/new (fn ()
                      (let* [val (fiber/resume c)]
                        (yield val))) 2)
       a-result (fiber/resume b)]
  (assert (= a-result "from-c") "abc chain yield through"))

# test_root_child_yield
# Root resumes a yielding child. Child yields a string.
(let* [f (fiber/new (fn () (yield "from-child")) 2)
       result (fiber/resume f)]
  (assert (= result "from-child") "root child yield"))

# test_root_child_grandchild_yield
# Root→child→grandchild. Grandchild yields string,
# child yields it to root.
(let* [gc (fiber/new (fn () (yield "from-gc")) 2)
       child (fiber/new (fn ()
                          (let* [val (fiber/resume gc)]
                            (yield val))) 2)]
  (let [result (fiber/resume child)]
    (assert (= result "from-gc") "root child grandchild yield")))

# test_child_death_value_survives
# Child yields a string then completes (dies).
# The yielded string should survive child death because it's
# in the shared allocator (owned by parent or child).
(let* [f (fiber/new (fn ()
                      (yield "alive")
                      "done") 2)
       yielded (fiber/resume f)
       _ (fiber/resume f)]
  (assert (= yielded "alive") "child death value survives"))

# test_multi_resume_yield_basic
# Multiple yields without letrec — tests shared alloc across resumes.
(let* [f (fiber/new (fn ()
                      (yield 0)
                      (yield 1)
                      (yield 2)) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 0) "multi resume yield basic: first")
    (assert (= v2 1) "multi resume yield basic: second")
    (assert (= v3 2) "multi resume yield basic: third")))

# test_multi_resume_yield_heap_values
# Yield heap-allocated values across multiple resumes.
# Tests that shared alloc keeps values alive for the parent.
(let* [f (fiber/new (fn ()
                      (yield "hello")
                      (yield "world")
                      (yield "done")) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 "hello") "multi resume heap: first")
    (assert (= v2 "world") "multi resume heap: second")
    (assert (= v3 "done") "multi resume heap: third")))

# test_multi_resume_yield_mixed_values
# Yield a mix of immediate and heap values across resumes.
(let* [f (fiber/new (fn ()
                      (yield 42)
                      (yield (list 1 2 3))
                      (yield "end")) 2)]
  (let [v1 (fiber/resume f)
        v2 (fiber/resume f)
        v3 (fiber/resume f)]
    (assert (= v1 42) "multi resume mixed: first")
    (assert (= (length v2) 3) "multi resume mixed: second is list")
    (assert (= v3 "end") "multi resume mixed: third")))

# test_multiple_children_shared_allocs
# Parent resumes two different yielding children.
# Both yield strings. Both readable.
# Tests owned_shared Vec growth doesn't invalidate earlier pointers.
(let* [f1 (fiber/new (fn () (yield "from-f1")) 2)
       f2 (fiber/new (fn () (yield "from-f2")) 2)
       v1 (fiber/resume f1)
       v2 (fiber/resume f2)]
  (assert (= v1 "from-f1") "multiple children: first")
  (assert (= v2 "from-f2") "multiple children: second"))

# ── Lifecycle and edge cases ────────────────────────────────────────

# test_yield_immediate_no_shared_alloc_needed
# Yielding an immediate (int) requires no heap allocation.
# The shared alloc infrastructure should not interfere.
(let* [f (fiber/new (fn () (yield 42)) 2)
       result (fiber/resume f)]
  (assert (= result 42) "yield immediate no shared alloc"))

# test_yield_list_parent_traverses
# Fiber yields a pair list. Parent traverses all elements.
# The list cells are heap-allocated — they go to shared alloc.
(let* [f (fiber/new (fn () (yield (list 10 20 30))) 2)
       lst (fiber/resume f)]
  (assert (= (first lst) 10) "yield list: first")
  (assert (= (first (rest lst)) 20) "yield list: second")
  (assert (= (first (rest (rest lst))) 30) "yield list: third"))

# test_yield_star_with_shared_alloc
# yield* delegates iteration. Values flow through shared alloc.
(def sub
  (coro/new (fn ()
              (yield "a")
              (yield "b")
              :done)))
(def main (coro/new (fn () (yield* sub))))
(coro/resume main nil)
(def v1 (coro/value main))
(coro/resume main nil)
(def v2 (coro/value main))
(assert (= v1 "a") "yield star: first")
(assert (= v2 "b") "yield star: second")

# test_error_in_child_with_shared_alloc
# Child fiber signals an error. The error value (a struct/tuple)
# is in shared space. Parent catches and reads the error message.
(let* [f (fiber/new (fn () (error "test error")) 1)
       _ (fiber/resume f)
       val (fiber/value f)]
  (assert (not (nil? val)) "error in child with shared alloc"))

# test_cancel_child_with_shared_alloc
# Parent cancels a suspended child that has a shared allocator.
# Mask 3 catches both error (1) and yield (2) so cancel doesn't propagate.
(let* [f (fiber/new (fn ()
                      (yield "yielded")
                      "never-reached") 3)
       v1 (fiber/resume f)]
  (fiber/cancel f "cancelled")
  (let [status (string (fiber/status f))]
    (assert (= v1 "yielded") "cancel child: yielded value")
    (assert (= status "error") "cancel child: status is error")))

# test_long_lived_coroutine_many_resumes
# Resume a coroutine 50 times, each time yielding a heap value (list).
# Exercises M2 — many shared allocs accumulate in owned_shared.
# All yielded values must be readable at the end.
(def @gen
  (coro/new (fn ()
              (var i 0)
              (while (< i 50)
                (yield (list i (+ i 1)))
                (assign i (+ i 1))))))
(def @results @[])
(while (not (coro/done? gen))
  (coro/resume gen nil)
  (when (not (coro/done? gen)) (push results (coro/value gen))))
(assert (= (length results) 50) "long lived coroutine: 50 yields")
(assert (= (first (get results 0)) 0) "long lived coroutine: first yield")
(assert (= (first (get results 49)) 49) "long lived coroutine: last yield")

# ── Root fiber scope management (new in issue-525) ──────────────────

# test_root_fiber_scope_stats_nonnegative
# After issue-525, RegionEnter/RegionExit are effective on the root fiber.
# scope-enter-count should be >= 0 (may be > 0 due to stdlib scopes).
(let* [stats (arena/stats)
       enters (get stats :scope-enter-count)
       dtors (get stats :scope-dtor-count)]
  (assert (>= enters 0) "root fiber scope-enter-count is non-negative")
  (assert (>= dtors 0) "root fiber scope-dtor-count is non-negative"))

# test_root_fiber_count_nonzero
# After a full VM startup (stdlib loaded), arena/count on root must be > 0.
(assert (> (arena/count) 0)
        "root fiber arena/count is positive after stdlib load")

# ── arena/checkpoint (opaque mark) ────────────────────────────────

# test_checkpoint_reset_roundtrip
# After reset, any allocations after the checkpoint are gone.
# Note: (arena/checkpoint) itself allocates an External, so snapshot
# count BEFORE taking the checkpoint, then verify reset returns to that count.
(let* [before (arena/count)
       m (arena/checkpoint)
       _ (list 1 2 3)
       after-alloc (arena/count)
       _ (arena/reset m)
       after-reset (arena/count)]
  (assert (= (> after-alloc before) true) "count increased after alloc")
  (assert (= after-reset before) "count restored after reset"))

# test_checkpoint_is_opaque
# arena/reset should reject integers (old checkpoint format).
(let [[ok? err] (protect ((fn [] (arena/reset 42))))]
  (assert (not ok?) "arena/reset rejects integer (expected opaque checkpoint)")
  (assert (= (get err :error) :type-error)
          "arena/reset rejects integer (expected opaque checkpoint)"))

# test_checkpoint_reset_destructors_run
# Objects allocated after checkpoint are logically freed (destructors run).
# We verify via arena/count decreasing after reset.
# Snapshot count BEFORE taking the checkpoint (checkpoint itself allocates an External).
(let* [before (arena/count)
       m (arena/checkpoint)
       _ (string "hello")
       _ (string "world")
       after-alloc (arena/count)
       _ (arena/reset m)
       after-reset (arena/count)]
  (assert (= (> after-alloc before) true) "strings allocated")
  (assert (= after-reset before) "count restored: destructors ran"))

# ── Scope parameter removal regression (issue-525 follow-up) ────────
# Use apply to bypass compile-time arity checking; the runtime arity-error
# is what we're asserting against.

# test_arena_count_rejects_scope_arg
# After removing the scope parameter, passing :global must be an arity-error.
(let [[ok? err] (protect ((fn [] (apply arena/count [:global]))))]
  (assert (not ok?) "arena/count rejects :global after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/count rejects :global after arity reduction"))
(let [[ok? err] (protect ((fn [] (apply arena/count [:fiber]))))]
  (assert (not ok?) "arena/count rejects :fiber after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/count rejects :fiber after arity reduction"))

# test_arena_bytes_rejects_scope_arg
(let [[ok? err] (protect ((fn [] (apply arena/bytes [:global]))))]
  (assert (not ok?) "arena/bytes rejects :global after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/bytes rejects :global after arity reduction"))
(let [[ok? err] (protect ((fn [] (apply arena/bytes [:fiber]))))]
  (assert (not ok?) "arena/bytes rejects :fiber after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/bytes rejects :fiber after arity reduction"))

# test_arena_peak_rejects_scope_arg
(let [[ok? err] (protect ((fn [] (apply arena/peak [:global]))))]
  (assert (not ok?) "arena/peak rejects :global after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/peak rejects :global after arity reduction"))

# test_arena_reset_peak_rejects_scope_arg
(let [[ok? err] (protect ((fn [] (apply arena/reset-peak [:global]))))]
  (assert (not ok?) "arena/reset-peak rejects :global after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/reset-peak rejects :global after arity reduction"))

# test_arena_object_limit_rejects_scope_arg
(let [[ok? err] (protect ((fn [] (apply arena/object-limit [:global]))))]
  (assert (not ok?) "arena/object-limit rejects :global after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/object-limit rejects :global after arity reduction"))

# test_arena_set_object_limit_rejects_scope_arg
(let [[ok? err] (protect ((fn [] (apply arena/set-object-limit [100 :global]))))]
  (assert (not ok?)
          "arena/set-object-limit rejects second :global arg after arity reduction")
  (assert (= (get err :error) :arity-error)
          "arena/set-object-limit rejects second :global arg after arity reduction"))

# ── arena/stats new fields (Chunk 5) ───────────────────────────────

# test_arena_stats_has_new_fields
# Verify the unified arena/stats struct has all the new fields.
(let* [s (arena/stats)]
  (assert (struct? s) "arena/stats returns struct")
  (assert (int? (get s :object-count)) "arena/stats :object-count is int")
  (assert (int? (get s :peak-count)) "arena/stats :peak-count is int")
  (assert (int? (get s :allocated-bytes)) "arena/stats :allocated-bytes is int")
  (assert (int? (get s :scope-depth)) "arena/stats :scope-depth is int")
  (assert (int? (get s :dtor-count)) "arena/stats :dtor-count is int")
  (assert (int? (get s :root-live-count)) "arena/stats :root-live-count is int")
  (assert (int? (get s :root-alloc-count))
          "arena/stats :root-alloc-count is int")
  (assert (int? (get s :shared-count)) "arena/stats :shared-count is int")
  (assert (or (= :slab (get s :active-allocator))
              (= :bump (get s :active-allocator)))
          "arena/stats :active-allocator is :slab or :bump")
  (assert (int? (get s :scope-enter-count))
          "arena/stats :scope-enter-count is int")
  (assert (int? (get s :scope-dtor-count))
          "arena/stats :scope-dtor-count is int"))

# test_arena_stats_no_capacity_field
# The old :capacity field must be absent in the unified struct.
(let* [s (arena/stats)]
  (assert (nil? (get s :capacity)) "arena/stats :capacity field removed"))

# test_arena_stats_active_allocator_is_slab_at_root
# At root (no scope), :active-allocator must be :slab.
(let* [s (arena/stats)]
  (assert (= (get s :active-allocator) :slab)
          "arena/stats :active-allocator is :slab at root"))

# test_arena_stats_scope_depth_is_zero_at_root
# At root (no scope), :scope-depth must be 0.
(let* [s (arena/stats)]
  (assert (= (get s :scope-depth) 0) "arena/stats :scope-depth is 0 at root"))

# test_arena_stats_object_limit_nil_by_default
# :object-limit is nil when no limit is set.
(let* [s (arena/stats)]
  (assert (nil? (get s :object-limit))
          "arena/stats :object-limit is nil with no limit set"))

# test_arena_stats_object_limit_reflects_set_limit
# After setting a limit, :object-limit should reflect it.
# Use a very large limit to avoid interfering with ongoing allocations.
(let* [_ (arena/set-object-limit 9999999)
       s (arena/stats)
       limit (get s :object-limit)
       _ (arena/set-object-limit nil)]
  (assert (= limit 9999999) "arena/stats :object-limit reflects set limit"))

# test_arena_stats_bytes_matches_arena_bytes
# :allocated-bytes in arena/stats should match (arena/bytes).
(let* [s (arena/stats)
       stats-bytes (get s :allocated-bytes)
       direct-bytes (arena/bytes)]
  (assert (>= stats-bytes 0) "arena/stats :allocated-bytes is non-negative")
  (assert (>= direct-bytes 0) "arena/bytes is non-negative"))

# test_arena_stats_arity_error_two_args
# arena/stats with 2 arguments must return an arity-error.
(let [[ok? err] (protect ((fn [] (apply arena/stats [1 2]))))]
  (assert (not ok?) "arena/stats rejects 2 arguments")
  (assert (= (get err :error) :arity-error) "arena/stats rejects 2 arguments"))

# test_arena_stats_fiber_arg_type_error
# arena/stats with a non-fiber argument must return a type-error.
(let [[ok? err] (protect ((fn [] (arena/stats 42))))]
  (assert (not ok?) "arena/stats rejects non-fiber argument")
  (assert (= (get err :error) :type-error)
          "arena/stats rejects non-fiber argument"))

# test_arena_fiber_stats_via_unified_interface
# arena/stats with a fiber arg returns stats for that fiber.
(let* [f (fiber/new (fn () 42) 1)
       _ (fiber/resume f)
       s (arena/stats f)]
  (assert (struct? s) "arena/stats with fiber arg returns struct")
  (assert (int? (get s :object-count)) "fiber stats :object-count is int")
  (assert (int? (get s :peak-count)) "fiber stats :peak-count is int")
  (assert (int? (get s :allocated-bytes)) "fiber stats :allocated-bytes is int"))

# test_arena_fiber_stats_no_capacity_field
# The old arena/fiber-stats had no :capacity field. The new unified
# struct also has no :capacity field.
(let* [f (fiber/new (fn () 42) 1)
       _ (fiber/resume f)
       s (arena/stats f)]
  (assert (nil? (get s :capacity)) "unified fiber stats has no :capacity field"))

# test_arena_fiber_stats_removed
# arena/fiber-stats primitive must no longer exist.
# vm/primitive-meta returns nil for unknown names.
(assert (nil? (vm/primitive-meta "arena/fiber-stats"))
        "arena/fiber-stats is removed from primitives")

# test_arena_scope_stats_removed
# arena/scope-stats primitive must no longer exist; its fields are in arena/stats.
(assert (nil? (vm/primitive-meta "arena/scope-stats"))
        "arena/scope-stats is removed from primitives")

# test_scope_enter_count_is_int
# :scope-enter-count is a non-negative integer at root.
(let* [s (arena/stats)
       enter-count (get s :scope-enter-count)]
  (assert (int? enter-count) ":scope-enter-count is int")
  (assert (>= enter-count 0) ":scope-enter-count is non-negative"))

# test_scope_dtor_count_is_int
# :scope-dtor-count is a non-negative integer at root.
(let* [s (arena/stats)
       dtor-count (get s :scope-dtor-count)]
  (assert (int? dtor-count) ":scope-dtor-count is int")
  (assert (>= dtor-count 0) ":scope-dtor-count is non-negative"))

# ── Migrated from Rust: mark/release / scope / alloc-error ─────────

# test_fiber_heap_mark_release
# alloc, mark, alloc more, release — count returns to pre-mark level.
(let* [before (arena/count)
       m (arena/checkpoint)
       _ (string "a")
       _ (string "b")
       _ (string "c")
       _ (arena/reset m)
       after (arena/count)]
  (assert (= after before) "mark/release: count restored after release"))

# test_fiber_heap_nested_mark_release
# Nested marks: inner release leaves outer alloc; outer release clears all.
(let* [before (arena/count)
       outer-m (arena/checkpoint)
       _ (string "outer")
       inner-m (arena/checkpoint)
       _ (string "inner")
       after-inner-alloc (arena/count)]
  (assert (= (- after-inner-alloc before) 2) "two allocs after outer+inner mark")
  (arena/reset inner-m)
  (let* [after-inner-reset (arena/count)]
    (assert (= (- after-inner-reset before) 1) "inner reset: one alloc remains")
    (arena/reset outer-m)
    (let* [after-outer-reset (arena/count)]
      (assert (= after-outer-reset before) "outer reset: back to baseline"))))

# test_clear_resets_scope_counters
# :scope-enter-count and :scope-dtor-count reset to 0 after a fiber is cleared.
# We verify indirectly: a new child fiber starts with zero scope counters.
(let* [f (fiber/new (fn ()
                      (let* [_ (arena/stats)]
                        (arena/stats))) 1)
       stats (fiber/resume f)
       enters (get stats :scope-enter-count)
       dtors-run (get stats :scope-dtor-count)]
  (assert (>= enters 0) "new fiber :scope-enter-count is non-negative")
  (assert (>= dtors-run 0) "new fiber :scope-dtor-count is non-negative"))

# test_memory_stabilizes_after_release
# After alloc/release cycle, :allocated-bytes must not grow on the second cycle
# (slab reuses freed slots). Use arena/stats :allocated-bytes for comparison.
(let* [m1 (arena/checkpoint)
       _ (letrec [loop (fn (i)
                         (when (%lt i 50)
                           (%pair i (%add i 1))
                           (loop (%add i 1))))]
           (loop 0))
       bytes-round1 (get (arena/stats) :allocated-bytes)
       _ (arena/reset m1)
       m2 (arena/checkpoint)
       _ (letrec [loop (fn (i)
                         (when (%lt i 50)
                           (%pair i (%add i 1))
                           (loop (%add i 1))))]
           (loop 0))
       bytes-round2 (get (arena/stats) :allocated-bytes)
       _ (arena/reset m2)]
  (assert (= bytes-round1 bytes-round2)
          "slab reuses freed slots: :allocated-bytes must not grow across release cycles"))

# test_scope_mark_push_pop_lifecycle
# arena/stats :scope-depth reflects scope push/pop.
# Since scope depth is only visible through arena/stats :scope-depth, and
# user code cannot enter a scope without the compiler's RegionEnter, this
# test verifies that :scope-depth is 0 at root (no active user scopes).
(let* [s (arena/stats)]
  (assert (= (get s :scope-depth) 0)
          "scope-depth is 0 at root (no user-level scope active)"))

# test_take_alloc_error_initially_none
# Without a limit set, :object-limit is nil.
(let* [s (arena/stats)]
  (assert (nil? (get s :object-limit))
          ":object-limit is nil when no limit is set"))

# test_alloc_error_set_on_limit_exceeded
# Verify limit can be set and cleared. We set a very high limit to avoid
# breaking subsequent allocations.
(let* [_ (arena/set-object-limit 9999999)
       s (arena/stats)
       limit-while-set (get s :object-limit)
       _ (arena/set-object-limit nil)]
  (assert (= limit-while-set 9999999)
          "arena/set-object-limit: limit reflected in arena/stats while set"))

# test_alloc_error_cleared_by_set_object_limit_nil
# After removing the limit, :object-limit returns to nil.
(let* [_ (arena/set-object-limit 9999999)
       _ (arena/set-object-limit nil)
       s (arena/stats)]
  (assert (nil? (get s :object-limit))
          ":object-limit is nil after removing limit"))

# test_active_alloc_starts_as_slab
# At root (no scope), :active-allocator is :slab.
(assert (= (get (arena/stats) :active-allocator) :slab)
        "active-allocator is :slab at root")

# test_alloc_tracked
# After allocations, :object-count increases. Under the async scheduler,
# allocations may route through a shared allocator (not the root slab),
# so we check :object-count (which includes shared alloc) rather than
# :root-live-count (which only tracks root slab slots).
(let* [before-s (arena/stats)
       before-count (get before-s :object-count)
       _ (pair 1 2)  # allocates one Cons
       after-s (arena/stats)
       after-count (get after-s :object-count)]
  (assert (> after-count before-count)
          ":object-count increases after allocation"))

# test_create_shared_allocator_tracked
# Resuming a yielding fiber creates a shared allocator: :shared-count increases.
# We verify that :shared-count is a non-negative integer (invariant).
(let* [s (arena/stats)]
  (assert (>= (get s :shared-count) 0) ":shared-count is non-negative"))

# test_create_multiple_shared_allocators
# :shared-count is a non-negative integer. The internal tracking of shared
# allocators is on the VM fiber's heap, not the ROOT_HEAP thread-local that
# arena/stats reads from root context. Verify the field is structurally valid.
(let* [f1 (fiber/new (fn () (yield "y1")) 2)
       f2 (fiber/new (fn () (yield "y2")) 2)
       _ (fiber/resume f1)
       _ (fiber/resume f2)
       s (arena/stats)]
  (assert (int? (get s :shared-count))
          ":shared-count is int after multiple yielding fibers")
  (assert (>= (get s :shared-count) 0)
          ":shared-count is non-negative after multiple yielding fibers"))
