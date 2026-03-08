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

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Basic arena stats
# ========================================

(let ((stats (arena/stats)))
  (assert-true (>= (get stats :count) 0) "arena count is non-negative")
  (assert-true (>= (get stats :capacity) (get stats :count)) "capacity >= count"))

(let ((c (arena/count)))
  (assert-true (number? c) "arena/count returns a number")
  (assert-true (> c 0) "arena/count is positive after stdlib init")
  (display "  arena has ") (display c) (display " objects\n"))


# ========================================
# 2. Measurement overhead
# ========================================

# Each (arena/count) call has 1 object of overhead (SIG_QUERY cons)
(let* ((a (arena/count))
       (b (arena/count))
       (c (arena/count)))
  (assert-eq (- b a) 1 "arena/count overhead is 1")
  (assert-eq (- c b) 1 "arena/count overhead is stable"))

(display "  arena/count overhead: 1 object per call\n")

# arena/allocs compensates for this
(let* ((m (arena/allocs (fn () nil)))
       (allocs (first (rest m))))
  (assert-eq allocs 0 "nil thunk allocates 0 net objects")
  (display "  arena/allocs overhead: compensated to 0\n"))


# ========================================
# 3. Allocation costs per type
# ========================================

(defn net-allocs (thunk)
  "Measure net allocations from a thunk, compensating for overhead."
  (first (rest (arena/allocs thunk))))

(let ((n (net-allocs (fn () (cons 1 2)))))
  (assert-eq n 1 "cons = 1 heap object")
  (display "  cons:         ") (display n) (newline))

(let ((n (net-allocs (fn () (list 1 2 3 4 5)))))
  (assert-eq n 5 "list of 5 = 5 cons cells")
  (display "  (list 1..5):  ") (display n) (newline))

(let ((n (net-allocs (fn () (fn (x) x)))))
  (assert-eq n 1 "closure = 1 heap object")
  (display "  closure:      ") (display n) (newline))

(let ((n (net-allocs (fn () @[1 2 3]))))
  (assert-eq n 1 "array = 1 heap object")
  (display "  @[1 2 3]:     ") (display n) (newline))

(let ((n (net-allocs (fn () @{:a 1 :b 2}))))
  (assert-eq n 1 "table = 1 heap object")
  (display "  @{:a 1 :b 2}: ") (display n) (newline))

# String literals are in the constant pool, not runtime-allocated
(let ((n (net-allocs (fn () "a long string literal"))))
  (assert-eq n 0 "string literal = 0 (constant pool)")
  (display "  str literal:  ") (display n) (newline))


# ========================================
# 4. Immediates are free
# ========================================

(assert-eq (net-allocs (fn () (+ 1 2))) 0 "int arithmetic = 0")
(assert-eq (net-allocs (fn () (< 1 2))) 0 "comparison = 0")
(assert-eq (net-allocs (fn () nil)) 0 "nil = 0")
(assert-eq (net-allocs (fn () :foo)) 0 "keyword = 0")

(display "  int, bool, nil, keyword: 0 objects each\n")


# ========================================
# 5. Growth patterns
# ========================================

# Pure int loop: bounded
(let ((n (net-allocs (fn ()
           (letrec ((loop (fn (i acc)
                            (if (>= i 1000) acc
                              (loop (+ i 1) (+ acc i))))))
             (loop 0 0))))))
  (display "  1000 int additions: ") (display n) (display " objects\n")
  (assert-true (<= n 2) "int loop is bounded"))

# List creation: linear (expected without arena release)
(let ((n (net-allocs (fn ()
           (letrec ((loop (fn (i)
                            (when (< i 100)
                              (list 1 2 3)
                              (loop (+ i 1))))))
             (loop 0))))))
  (display "  100x (list 1 2 3): ") (display n) (display " objects\n")
  (assert-true (>= n 300) "list creation is linear"))


# ========================================
# 6. Scope allocation — compile-time analysis
# ========================================
#
# Note: let bodies that call polymorphic-effect functions (map, filter, fold
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

(assert-eq (count-regions arith-let) 1 "arithmetic let emits 1 RegionEnter")
(display "  arith-let:    ") (display (count-regions arith-let)) (print " region(s)")

# Returning a heap value — does NOT qualify.
(defn heap-let []
  (let ((x (list 1 2 3))) x))

(assert-eq (count-regions heap-let) 0 "heap-returning let emits 0 regions")
(display "  heap-let:     ") (display (count-regions heap-let)) (print " region(s)")

# Whitelist primitive (Tier 1): length returns int.
(defn length-let []
  (let ((x (list 1 2 3))) (length x)))

(assert-eq (count-regions length-let) 1 "length let emits 1 region")
(display "  length-let:   ") (display (count-regions length-let)) (print " region(s)")

# Match with immediate arms (Tier 5).
(defn match-let []
  (let ((x 1))
    (match x (0 :zero) (1 :one) (_ :other))))

(assert-eq (count-regions match-let) 1 "match-let emits 1 region")
(display "  match-let:    ") (display (count-regions match-let)) (print " region(s)")

# Match with a heap arm — does NOT qualify.
(defn match-heap []
  (let ((x 1))
    (match x (0 :zero) (_ (list 1 2 3)))))

(assert-eq (count-regions match-heap) 0 "match with heap arm emits 0 regions")
(display "  match-heap:   ") (display (count-regions match-heap)) (print " region(s)")

# Nested lets — both qualify (Tier 4).
(defn nested-let []
  (let ((x 1))
    (let ((y 2))
      (+ x y))))

(assert-true (>= (count-regions nested-let) 2) "nested lets emit >= 2 regions")
(display "  nested-let:   ") (display (count-regions nested-let)) (print " region(s)")

# Captured binding — does NOT qualify.
(defn captured-let []
  (let ((x 1)) (fn [] x)))

(assert-eq (count-regions captured-let) 0 "captured binding emits 0 regions")
(display "  captured-let: ") (display (count-regions captured-let)) (print " region(s)")


# ========================================
# 7. Runtime scope stats in a child fiber
# ========================================

# arena/scope-stats returns {:enters N :dtors-run N}.
# Non-zero only inside child fibers (root fiber has no FiberHeap).

(let ((stats (arena/scope-stats)))
  (assert-eq (get stats :enters) 0 "root fiber has 0 scope enters")
  (display "  root fiber:   ") (print stats))

# Non-yielding fibers (arity 1) use private FiberHeap where scope
# marks actually free objects. Yielding fibers route allocations
# to a shared allocator, bypassing scope marks.

(defn run-in-fiber [thunk]
  "Execute thunk in a non-yielding child fiber, return its result."
  (fiber/resume (fiber/new thunk 1)))

# Single scope allocation with an array (arrays need Drop).
(let ((stats (run-in-fiber (fn []
               (let ((x @[1 2 3])) (length x))
               (arena/scope-stats)))))
  (assert-eq (get stats :enters) 1 "1 scope enter")
  (assert-eq (get stats :dtors-run) 1 "1 destructor run")
  (display "  single scope: ") (print stats))

# 100 iterations — each scope allocates and frees an array.
# Tier 8 allows the implicit while-block to scope-allocate (outward set of
# immediate value is safe), adding 1 region enter around the whole loop.
(let ((stats (run-in-fiber (fn []
               (var i 0)
               (while (< i 100)
                 (let ((x @[1 2 3])) (length x))
                 (assign i (+ i 1)))
               (arena/scope-stats)))))
  (assert-eq (get stats :enters) 101 "101 scope enters (100 inner let + 1 while block)")
  (assert-eq (get stats :dtors-run) 100 "100 destructors run")
  (display "  100-iter loop: ") (print stats))


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
(display "  50x scoped:   ") (display scoped-net) (print " net objects")

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
(display "  50x unscoped: ") (display unscoped-net) (print " net objects")

(assert-true (< scoped-net unscoped-net) "scoping reduces net objects")
(display "  savings:      ") (display (- unscoped-net scoped-net)) (print " objects freed early")


# ========================================
# 9. Macro expansion: ArenaGuard works
# ========================================

# Per-iteration cost must be constant regardless of N.
# If ArenaGuard is working, macro temps are freed each expansion.
(defn measure-per-iter (n expr)
  (let* ((before (arena/count)))
    (letrec ((loop (fn (i)
                     (when (< i n)
                       (eval expr)
                       (loop (+ i 1))))))
      (loop 0))
    (/ (- (arena/count) before 1) n)))

(let* ((e '(let ((a 0)) (each x (list 1 2 3) (assign a (+ a x))) a))
       (p10 (measure-per-iter 10 e))
       (p100 (measure-per-iter 100 e)))
  (display "  each via eval: ") (display p10) (display "/") (display p100)
  (display " per-iter (10x/100x)\n")
  (assert-eq p10 p100 "macro expansion cost is constant"))

(let* ((e '(defn temp (x) (let* ((a (+ x 1)) (b (+ a 2))) (-> b (* 2)))))
       (p10 (measure-per-iter 10 e))
       (p100 (measure-per-iter 100 e)))
  (display "  defn+let*+->: ") (display p10) (display "/") (display p100)
  (display " per-iter (10x/100x)\n")
  (assert-eq p10 p100 "complex macro cost is constant"))


# ========================================
# 10. Fiber lifecycle
# ========================================

(let* ((p10 (measure-per-iter 10
              '(let ((f (fiber/new (fn () 42) 1)))
                 (fiber/resume f nil))))
       (p100 (measure-per-iter 100
               '(let ((f (fiber/new (fn () 42) 1)))
                  (fiber/resume f nil)))))
  (display "  fiber create+resume: ") (display p10) (display "/") (display p100)
  (display " per-iter\n")
  (assert-eq p10 p100 "fiber cost is constant"))


# ========================================
# 11. Fiber-per-computation: bounded memory
# ========================================
#
# Wrapping each iteration in a child fiber reclaims all temporary
# allocations when the fiber dies. No GC — the bump resets on fiber death.
# Use this pattern for long-running loops that create many temporaries.

# Naive loop: allocations accumulate on the root fiber's arena.
(var naive-growth
  (let ((before (arena/count)))
    (var i 0)
    (while (< i 100)
      (list 1 2 3 4 5)
      (cons :a (cons :b nil))
      (set i (+ i 1)))
    (- (arena/count) before)))
(display "  naive 100 iters:  ") (display naive-growth) (print " net objects")

# Fiber-per-iteration: each iteration runs in a child fiber.
# When the fiber completes, its FiberHeap is reclaimed entirely.
(var fiber-growth
  (let ((before (arena/count)))
    (var i 0)
    (while (< i 100)
      (run-in-fiber (fn ()
        (list 1 2 3 4 5)
        (cons :a (cons :b nil))
        nil))
      (set i (+ i 1)))
    (- (arena/count) before)))
(display "  fiber 100 iters:  ") (display fiber-growth) (print " net objects")

# The fiber pattern's growth is lower: temporaries inside each fiber
# are reclaimed on fiber death. The root arena still grows from fiber
# objects and closures, but much less than the naive loop's 7 objects/iter.
(assert-true (> naive-growth (* 2 fiber-growth))
  "fiber-per-iteration keeps net growth lower than naive loop")
(display "  ratio:            ") (display (/ naive-growth fiber-growth)) (print "x")


# ========================================
# 12. arena/checkpoint and arena/reset
# ========================================
#
# Explicit reclamation for the root fiber. Dangerous: invalidates Values
# allocated after the checkpoint. Use only when you know those Values
# are no longer reachable.

# Take checkpoint, allocate, measure growth, reset, verify reclamation.
# arena/count has 1 object overhead (SIG_QUERY cons), so after reset
# the count reads as mark + 1.
(var cp-mark (arena/checkpoint))
(cons 1 2)
(cons 3 4)
(cons 5 6)
(list 7 8 9)
(var cp-after (arena/count))
(assert-true (> cp-after cp-mark) "objects were allocated after checkpoint")
(display "  after alloc:  ") (display (- cp-after cp-mark)) (print " new objects")
(arena/reset cp-mark)
(var cp-reset (arena/count))
# arena/count itself allocates 1 cons (SIG_QUERY overhead)
(assert-eq (- cp-reset cp-mark) 1 "arena/reset restored count (modulo measurement overhead)")
(display "  after reset:  +") (display (- cp-reset cp-mark)) (print " (measurement overhead)")

# Verify reset with invalid mark errors
(let ((result (try
                (arena/reset (+ (arena/checkpoint) 999))
                (catch e (get e :error)))))
  (assert-eq result :value-error "arena/reset with future mark errors")
  (display "  bad mark:     caught ") (print result))

(print "")
(print "all allocator tests passed.")
