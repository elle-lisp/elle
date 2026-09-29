(elle/epoch 13)
# audited: 2026-09-29
# A closure bound by def or let before a loop, and called inside it, survives every iteration.
# docs/impl/region/anchors.md
#
# The sidecar arms guardfree, so a read of a freed closure faults.
#
# Liveness extends a binding's last use across a loop when the binding is
# bound outside that loop. `compute_subtree_low` (src/hir/liveness.rs) makes
# "outside" a range test, `low[loop] <= order[scope] <= order[loop]`, which
# holds for a scope that encloses the loop and for one that precedes it as a
# sibling.
#
# The counter-factual: the test `order[scope] > order[loop]` recognizes only
# the enclosing scope. A `(def helper ...)` is a preceding sibling of the
# loop, with a SMALLER post-order index, so its last use stays inside the loop
# body and the lowerer frees the closure after the first iteration. The next
# allocation reuses the closure's slot, and a debug build trips the deref
# tag/object assert (value.tag=CLOSURE on a slot now holding an array).

## ── 1. def-bound closure, called across many iterations ─────────────────
(defn run-def []
  (def @helper (fn [x] (* x 2)))
  (def @i 0)
  (def @sum 0)
  (while (< i 5)
    ## Churn the slab each iteration so a freed closure's slot is reused,
    ## surfacing the use-after-free as a tag/object mismatch rather than
    ## silently reading stale-but-intact memory.
    (let [junk @{:a i :b (* i i) :c [i i i]}]
      (assign sum (+ sum (helper (get junk :a)))))
    (assign i (+ i 1)))
  sum)

(let [r (run-def)]
  (assert (= r 20) (concat "def-bound closure: expected 20, got " (string r))))
(println "  1. def-bound loop-invariant closure survived: ok")

## ── 2. let-bound closure (preceding the loop in let* body) ──────────────
## let* sequences bindings; the closure binding precedes the loop in the
## shared body, the same preceding-sibling shape as case 1.
(defn run-let []
  (let* [helper (fn [x] (+ x 100))
         acc (box 0)]
    (def @i 0)
    (while (< i 4)
      (let [junk [i i i]]
        (rebox acc (+ (unbox acc) (helper (get junk 0)))))
      (assign i (+ i 1)))
    (unbox acc)))

(let [r (run-let)]
  (assert (= r 406) (concat "let-bound closure: expected 406, got " (string r))))
(println "  2. let-bound loop-invariant closure survived: ok")

## ── 3. nested loops: closure must outlive the OUTER loop ────────────────
(defn run-nested []
  (def @helper (fn [a b] (+ a b)))
  (def @i 0)
  (def @total 0)
  (while (< i 3)
    (def @j 0)
    (while (< j 3)
      (let [junk @{:p i :q j}]
        (assign total (+ total (helper (get junk :p) (get junk :q)))))
      (assign j (+ j 1)))
    (assign i (+ i 1)))
  total)

## sum of (i+j) for i,j in 0..2  =  3 + 6 + 9  =  18
(let [r (run-nested)]
  (assert (= r 18) (concat "nested-loop closure: expected 18, got " (string r))))
(println "  3. closure survives nested loops: ok")

(println "loop-def-closure-uaf: all tests passed")
