#!/usr/bin/env elle

# Allocator — memory allocation and scope allocation introspection
#
# Demonstrates:
#   Arena introspection — (arena/count), (arena/stats), (arena/allocs)
#   Allocation costs — heap types vs immediates
#   Growth patterns — bounded vs linear allocation
#   Scope allocation — compile-time RegionEnter/RegionExit
#   Runtime scope stats — scope-stats inside child fibers
#   Escape analysis — which let/block patterns qualify for scope allocation
#
# Scope allocation frees heap objects at scope exit rather than waiting
# for fiber death. It works on non-yielding child fibers where
# allocations go to the private FiberHeap bump. The escape analysis
# in the lowerer decides which scopes qualify.



# ========================================
# 1. Basic arena stats
# ========================================

(let ((stats (arena/stats)))
  (assert (>= (get stats :object-count) 0) "arena object-count is non-negative")
  (assert (>= (get stats :allocated-bytes) 0) "allocated-bytes is non-negative"))

(let ((c (arena/count)))
  (assert (number? c) "arena/count returns a number")
  (assert (> c 0) "arena/count is positive after stdlib init")
  (print "  arena has ") (print c) (print " objects\n"))


# ========================================
# 2. Measurement overhead
# ========================================

# arena/count now operates directly on thread-local state (no SIG_QUERY overhead)
(let* ((a (arena/count))
       (b (arena/count))
       (c (arena/count)))
  (assert (= (- b a) 0) "arena/count has zero overhead")
  (assert (= (- c b) 0) "arena/count overhead is stable"))

(print "  arena/count overhead: 0 objects per call\n")

# arena/allocs compensates for this
(let* ((m (arena/allocs (fn () nil)))
       (allocs (rest m)))
  (assert (= allocs 0) "nil thunk allocates 0 net objects")
  (print "  arena/allocs overhead: compensated to 0\n"))


# ========================================
# 3. Allocation costs per type
# ========================================

(defn net-allocs (thunk)
  "Measure net allocations from a thunk."
  (rest (arena/allocs thunk)))

(let ((n (net-allocs (fn () (cons 1 2)))))
  (assert (= n 1) "cons = 1 heap object")
  (print "  cons:         ") (print n) (println))

(let ((n (net-allocs (fn () (list 1 2 3 4 5)))))
  (assert (= n 5) "list of 5 = 5 cons cells")
  (print "  (list 1..5):  ") (print n) (println))

(let ((n (net-allocs (fn () (fn (x) x)))))
  (assert (= n 1) "closure = 1 heap object")
  (print "  closure:      ") (print n) (println))

(let ((n (net-allocs (fn () @[1 2 3]))))
  (assert (= n 1) "array = 1 heap object")
  (print "  @[1 2 3]:     ") (print n) (println))

(let ((n (net-allocs (fn () @{:a 1 :b 2}))))
  (assert (= n 1) "@struct = 1 heap object")
  (print "  @{:a 1 :b 2}: ") (print n) (println))

# String literals are in the constant pool, not runtime-allocated
(let ((n (net-allocs (fn () "a long string literal"))))
  (assert (= n 0) "string literal = 0 (constant pool)")
  (print "  str literal:  ") (print n) (println))


# ========================================
# 4. Immediates are free
# ========================================

(assert (= (net-allocs (fn () (+ 1 2))) 0) "int arithmetic = 0")
(assert (= (net-allocs (fn () (< 1 2))) 0) "comparison = 0")
(assert (= (net-allocs (fn () nil)) 0) "nil = 0")
(assert (= (net-allocs (fn () :foo)) 0) "keyword = 0")

(print "  int, bool, nil, keyword: 0 objects each\n")


# ========================================
# 5. Growth patterns
# ========================================

# Pure int loop: bounded
(let ((n (net-allocs (fn ()
           (letrec ((loop (fn (i acc)
                            (if (>= i 1000) acc
                              (loop (+ i 1) (+ acc i))))))
             (loop 0 0))))))
  (print "  1000 int additions: ") (print n) (print " objects\n")
  (assert (<= n 2) "int loop is bounded"))

# List creation: linear (expected without arena release)
(let ((n (net-allocs (fn ()
           (letrec ((loop (fn (i)
                            (when (< i 100)
                              (list 1 2 3)
                              (loop (+ i 1))))))
             (loop 0))))))
  (print "  100x (list 1 2 3): ") (print n) (print " objects\n")
  (assert (>= n 300) "list creation is linear"))


# ========================================
# 6. Scope allocation — compile-time analysis
# ========================================
#
# Note: let bodies that call polymorphic-signal functions (map, filter, fold
# with callbacks) or user-defined functions cannot scope-allocate — the
# compiler cannot prove the result is immediate without interprocedural analysis.

# Count RegionEnter instructions in a closure's compiled bytecode.
(defn count-regions [f]
  "Count RegionEnter instructions in a closure's compiled bytecode."
  (var n 0)
  (each line in (disbit f)
    (when (string/contains? line "RegionEnter")
      (assign n (+ n 1))))
  n)

# Arithmetic result — qualifies for scope allocation.
(defn arith-let []
  (let ((a 1) (b 2)) (+ a b)))

(assert (= (count-regions arith-let) 1) "arithmetic let emits 1 RegionEnter")
(print "  arith-let:    ") (print (count-regions arith-let)) (println " region(s)")

# Returning a heap value — does NOT qualify.
(defn heap-let []
  (let ((x (list 1 2 3))) x))

(assert (= (count-regions heap-let) 0) "heap-returning let emits 0 regions")
(print "  heap-let:     ") (print (count-regions heap-let)) (println " region(s)")

# Whitelist primitive (Tier 1): length returns int.
(defn length-let []
  (let ((x (list 1 2 3))) (length x)))

(assert (= (count-regions length-let) 1) "length let emits 1 region")
(print "  length-let:   ") (print (count-regions length-let)) (println " region(s)")

# Match with immediate arms (Tier 5).
(defn match-let []
  (let ((x 1))
    (match x (0 :zero) (1 :one) (_ :other))))

(assert (= (count-regions match-let) 1) "match-let emits 1 region")
(print "  match-let:    ") (print (count-regions match-let)) (println " region(s)")

# Match with a heap arm — does NOT qualify.
(defn match-heap []
  (let ((x 1))
    (match x (0 :zero) (_ (list 1 2 3)))))

(assert (= (count-regions match-heap) 0) "match with heap arm emits 0 regions")
(print "  match-heap:   ") (print (count-regions match-heap)) (println " region(s)")

# Nested lets — both qualify (Tier 4).
(defn nested-let []
  (let ((x 1))
    (let ((y 2))
      (+ x y))))

(assert (>= (count-regions nested-let) 2) "nested lets emit >= 2 regions")
(print "  nested-let:   ") (print (count-regions nested-let)) (println " region(s)")

# Captured binding — does NOT qualify.
(defn captured-let []
  (let ((x 1)) (fn [] x)))

(assert (= (count-regions captured-let) 0) "captured binding emits 0 regions")
(print "  captured-let: ") (print (count-regions captured-let)) (println " region(s)")


# ========================================
# 7. Runtime scope stats in a child fiber
# ========================================

# arena/stats includes :scope-enter-count and :scope-dtor-count.
# After issue-525, the root fiber has a FiberHeap, so scope stats are
# tracked there too. :scope-enter-count may be > 0 from stdlib scope regions.

(let ((stats (arena/stats)))
  (assert (>= (get stats :scope-enter-count) 0) "root fiber scope-enter-count is non-negative")
  (print "  root fiber:   ") (println stats))

# Non-yielding fibers (arity 1) use private FiberHeap where scope
# marks actually free objects. Yielding fibers route allocations
# to a shared allocator, bypassing scope marks.

(defn run-in-fiber [thunk]
  "Execute thunk in a non-yielding child fiber, return its result."
  (fiber/resume (fiber/new thunk 1)))

# Single scope allocation with an array (arrays need Drop).
# Use before/after delta so escape analysis changes don't break the test.
(let ((stats (run-in-fiber (fn []
               (let* [[before (arena/stats)]]
                 (let ((x @[1 2 3])) (length x))
                 (let* [[after (arena/stats)]
                        [delta-enters (- (get after :scope-enter-count)
                                         (get before :scope-enter-count))]
                        [delta-dtors (- (get after :scope-dtor-count)
                                        (get before :scope-dtor-count))]]
                   (assert (>= delta-enters 1) "at least 1 scope enter")
                   (assert (>= delta-dtors 1) "at least 1 destructor run")
                   (print "  single scope: ") (println after)
                   after)))))))


# 100 iterations — each scope allocates and frees an array.
# Tier 8 allows the implicit while-block to scope-allocate (outward set of
# immediate value is safe), adding 1 region enter around the whole loop.
# Use before/after delta so escape analysis changes don't break the test.
(let ((stats (run-in-fiber (fn []
               (let* [[before (arena/stats)]]
                 (var i 0)
                 (while (< i 100)
                   (let ((x @[1 2 3])) (length x))
                   (assign i (+ i 1)))
                 (let* [[after (arena/stats)]
                        [delta-enters (- (get after :scope-enter-count)
                                         (get before :scope-enter-count))]
                        [delta-dtors (- (get after :scope-dtor-count)
                                        (get before :scope-dtor-count))]]
                   (assert (>= delta-enters 100) "at least 100 scope enters from inner let")
                   (assert (>= delta-dtors 100) "at least 100 destructors run")
                   (print "  100-iter loop: ") (println after)
                   after)))))))



# ========================================
# 8. Allocation savings: scoped vs unscoped
# ========================================

# Compare net live objects after a loop: scoped lets free arrays at
# scope exit, unscoped lets accumulate them until fiber death.

(var scoped-net
  (run-in-fiber (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 50)
      (let ((x @[1 2 3 4 5])) (length x))
      (assign i (+ i 1)))
    (- (arena/count) before))))
(print "  50x scoped:   ") (print scoped-net) (println " net objects")

# The `(assign last x)` stores a heap value outward, defeating while-block
# scope allocation (Tier 8 only allows immediate outward sets).
(var unscoped-net
  (run-in-fiber (fn []
    (var before (arena/count))
    (var i 0)
    (var last nil)
    (while (< i 50)
      (let ((x @[1 2 3 4 5])) (assign last x))
      (assign i (+ i 1)))
    (- (arena/count) before))))
(print "  50x unscoped: ") (print unscoped-net) (println " net objects")

(assert (< scoped-net unscoped-net) "scoping reduces net objects")
(print "  savings:      ") (print (- unscoped-net scoped-net)) (println " objects freed early")


# ========================================
# 9. Macro expansion: ArenaGuard works
# ========================================

# Per-iteration cost must be constant regardless of N for warm (cached) calls.
# Phase 2 of macro expansion uses an ArenaGuard to free transient allocations.
# The first expansion per macro compiles the transformer closure (no guard —
# the closure must survive to be cached). Subsequent expansions use the cached
# closure and are cheaper. We pre-warm the cache before measuring so both
# measurements reflect only the constant warm-path cost.
(defn measure-per-iter (n expr)
  (let* ((before (arena/count)))
    (letrec ((loop (fn (i)
                     (when (< i n)
                       (eval expr)
                       (loop (+ i 1))))))
      (loop 0))
    (/ (- (arena/count) before) n)))

(let* ((e '(let ((a 0)) (each x (list 1 2 3) (assign a (+ a x))) a))
       (_ (eval e))  # warm-up: compile transformer closure, amortized outside measurement
       (p5 (measure-per-iter 5 e))
       (p20 (measure-per-iter 20 e)))
  (print "  each via eval: ") (print p5) (print "/") (print p20)
  (print " per-iter (5x/20x)\n")
  (assert (= p5 p20) "macro expansion cost is constant after cache warm-up"))

(let* ((e '(defn temp (x) (let* ((a (+ x 1)) (b (+ a 2))) (-> b (* 2)))))
       (_ (eval e))  # warm-up: compile transformer closures for defn, let*, ->
       (p5 (measure-per-iter 5 e))
       (p20 (measure-per-iter 20 e)))
  (print "  defn+let*+->: ") (print p5) (print "/") (print p20)
  (print " per-iter (5x/20x)\n")
  (assert (= p5 p20) "complex macro cost is constant after cache warm-up"))


# ========================================
# 10. Fiber lifecycle
# ========================================

(let* ((p5 (measure-per-iter 5
              '(let ((f (fiber/new (fn () 42) 1)))
                 (fiber/resume f nil))))
       (p20 (measure-per-iter 20
               '(let ((f (fiber/new (fn () 42) 1)))
                  (fiber/resume f nil)))))
  (print "  fiber create+resume: ") (print p5) (print "/") (print p20)
  (print " per-iter\n")
  (assert (= p5 p20) "fiber cost is constant"))


# ========================================
# 11. Fiber-per-computation: bounded memory
# ========================================
#
# Wrapping each iteration in a child fiber reclaims all temporary
# allocations when the fiber dies. No GC — the bump resets on fiber death.
# Use this pattern for long-running loops that create many temporaries.

# Naive loop: each iteration assigns a heap value outward, defeating
# while-block scope allocation (Tier 8 only allows immediate outward sets).
# Objects accumulate on the root fiber's arena across iterations.
(var naive-last nil)
(var naive-before (arena/count))
(var i 0)
(while (< i 20)
  (assign naive-last (cons (list 1 2 3 4 5) naive-last))
  (assign i (+ i 1)))
(var naive-growth (- (arena/count) naive-before))
(print "  naive 20 iters:   ") (print naive-growth) (println " net objects")

# Fiber-per-iteration: each iteration runs in a child fiber.
# When the fiber completes, its FiberHeap is reclaimed entirely.
(var fiber-growth
  (let ((before (arena/count)))
    (var i 0)
    (while (< i 20)
      (run-in-fiber (fn ()
        (list 1 2 3 4 5)
        (cons :a (cons :b nil))
        nil))
      (assign i (+ i 1)))
    (- (arena/count) before)))
(print "  fiber 20 iters:   ") (print fiber-growth) (println " net objects")

# The fiber pattern's growth is lower: temporaries inside each fiber
# are reclaimed on fiber death. The naive loop escapes objects into acc
# so they accumulate (bypassing scope allocation), while the fiber loop
# only leaves the fiber object itself on the root arena.
(assert (> naive-growth (* 2 fiber-growth)) "fiber-per-iteration keeps net growth lower than naive loop")
(print "  savings:          ") (print (- naive-growth fiber-growth)) (println " fewer net objects")


# ========================================
# 12. arena/checkpoint and arena/reset
# ========================================
#
# Explicit reclamation for the root fiber. Dangerous: invalidates Values
# allocated after the checkpoint. Use only when you know those Values
# are no longer reachable.

# Take checkpoint, allocate, measure growth, reset, verify reclamation.
# arena/checkpoint returns an opaque external — use arena/count for arithmetic.
# Snapshot count before checkpoint (checkpoint itself allocates an External).
(var cp-before (arena/count))
(var cp-mark (arena/checkpoint))
(cons 1 2)
(cons 3 4)
(cons 5 6)
(list 7 8 9)
(var cp-after (arena/count))
# cp-after is cp-before + 1 (checkpoint External) + 4 (cons/list objects)
(assert (> cp-after cp-before) "objects were allocated after checkpoint")
(print "  after alloc:  ") (print (- cp-after cp-before)) (println " new objects")
(arena/reset cp-mark)
(var cp-reset (arena/count))
# after reset: checkpoint External and allocs are freed, back to cp-before
(assert (= cp-reset cp-before) "arena/reset restored count exactly")
(print "  after reset:  ") (print (- cp-reset cp-before)) (println " (no overhead)")

# Verify reset with invalid mark errors — a non-checkpoint value is rejected
(let ((result (try
                (arena/reset 999)
                (catch e (get e :error)))))
  (assert (= result :type-error) "arena/reset with non-checkpoint value errors")
  (print "  bad mark:     caught ") (println result))

(println "")
(println "all allocator tests passed.")
