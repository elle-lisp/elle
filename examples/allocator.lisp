# Allocator — scope allocation introspection
#
# Demonstrates:
#   Compile-time  — counting RegionEnter/RegionExit in compiled bytecode
#   Runtime stats — scope-stats inside child fibers
#   Qualifying    — which let/block patterns get scope-allocated
#   Non-qualifying — which patterns are rejected (and why)
#
# Scope allocation frees heap objects at scope exit rather than waiting
# for fiber death. It works on non-yielding child fibers where
# allocations go to the private FiberHeap bump. The escape analysis
# in the lowerer decides which scopes qualify.
#
# For compile-time rejection breakdown, run with:
#   ELLE_SCOPE_STATS=1 elle examples/allocator.lisp

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Counting regions in compiled bytecode
# ========================================

# Count RegionEnter instructions in a closure's compiled bytecode.
(defn count-regions [f]
  "Count RegionEnter instructions in a closure's compiled bytecode."
  (var n 0)
  (each line in (disbit f)
    (when (string/contains? line "RegionEnter")
      (set n (+ n 1))))
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
# 2. Runtime scope stats in a child fiber
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
                 (set i (+ i 1)))
               (arena/scope-stats)))))
  (assert-eq (get stats :enters) 101 "101 scope enters (100 inner let + 1 while block)")
  (assert-eq (get stats :dtors-run) 100 "100 destructors run")
  (display "  100-iter loop: ") (print stats))


# ========================================
# 3. Allocation savings: scoped vs unscoped
# ========================================

# Compare net live objects after a loop: scoped lets free arrays at
# scope exit, unscoped lets accumulate them until fiber death.

(var scoped-net
  (run-in-fiber (fn []
    (var before (arena/count))
    (var i 0)
    (while (< i 50)
      (let ((x @[1 2 3 4 5])) (length x))
      (set i (+ i 1)))
    (- (arena/count) before))))
(display "  50x scoped:   ") (display scoped-net) (print " net objects")

# The `(set last x)` stores a heap value outward, defeating while-block
# scope allocation (Tier 8 only allows immediate outward sets).
(var unscoped-net
  (run-in-fiber (fn []
    (var before (arena/count))
    (var i 0)
    (var last nil)
    (while (< i 50)
      (let ((x @[1 2 3 4 5])) (set last x))
      (set i (+ i 1)))
    (- (arena/count) before))))
(display "  50x unscoped: ") (display unscoped-net) (print " net objects")

(assert-true (< scoped-net unscoped-net) "scoping reduces net objects")
(display "  savings:      ") (display (- unscoped-net scoped-net)) (print " objects freed early")


(print "")
(print "all allocator tests passed.")
