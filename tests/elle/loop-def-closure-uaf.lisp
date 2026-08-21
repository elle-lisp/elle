(elle/epoch 12)
## tests/elle/loop-def-closure-uaf.lisp
##
## Regression: a closure bound by `def`/`let` BEFORE a loop and called
## INSIDE the loop must survive every iteration.
##
## Root cause (closed): the liveness iter-scope extension only extends a
## binding's last-use across a loop when the binding is bound *outside*
## that loop.  "Outside" was tested as `order[scope] > order[loop]`, which
## only recognises a binding whose scope node ENCLOSES the loop (a `let`
## with the loop in its body — an ancestor, so a larger post-order index).
## A `(def helper ...)` is a *preceding sibling* of the loop, not an
## ancestor: its post-order index is SMALLER than the loop's, so the test
## failed, last-use stayed inside the loop body, and the lowerer emitted a
## per-iteration DecrefRegion that freed the closure after iteration 1.
## The dangling closure's slab slot was reused by the next allocation,
## tripping the deref tag/object debug_assert (value.tag=CLOSURE on a slot
## now holding an array/struct) — the supervisor.lisp UAF, minimized.
##
## The fix tests subtree containment with a low-watermark range
## (`low[loop] <= order[scope] <= order[loop]`), which recognises BOTH the
## enclosing-ancestor and the preceding-sibling shapes as bound-outside.

## ── 1. def-bound closure, called across many iterations ─────────────────
(defn run-def []
  (def @helper (fn [x] (* x 2)))
  (def @i 0)
  (def @sum 0)
  (while (< i 5)  ## Churn the slab each iteration so a freed closure's slot is reused,
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
