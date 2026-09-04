(elle/epoch 12)
## tests/elle/region-capture-cell-loop-uaf.lisp
##
## Regression: a `@`-mutable captured local DEFINED INSIDE a loop and
## captured by a closure built in that loop must survive every iteration.
##
## Root cause (closed): such a binding is materialized as a `populate_env`
## env cell — a `StoreCapture` into a cell pre-allocated from
## `capture_locals_mask`, NOT a compiled `MakeCaptureCell`. The cell box is
## minted EXACTLY ONCE per activation (populate_env runs once), regardless of
## the loop the `def` sits in; re-executing the `def` each iteration only
## re-stores the cell's content. But the cell box's `DecrefCellRegion` was
## placed at the binding's last use, which — the only use being the in-loop
## capture — sits inside the loop body, so it fired every iteration. For a
## closure called in place and dying within the iteration, each iteration nets
## the box region -1 (closure capture-incref +1, closure free cascade -1,
## per-iteration DecrefCellRegion -1). The box, allocated once, was freed at
## the end of iteration 1; iteration 2 read the freed-and-recycled cell — a
## use-after-free (`as_capture_cell` tag/object mismatch under the plain VM, a
## cascade free under `--trace=guardfree`).
##
## The fix hoists a cell-release region's `decref_point` to the OUTERMOST
## enclosing While/Loop node (`hoist_cell_release_past_loops`), which the
## lowerer emits AFTER the loop — once per activation, matching the once-per-
## activation populate_env allocation. See docs/impl/region/bindings.md "Env
## cells in loops: release once per activation, not per iteration".
##
## This is the deferred Stage-2 captured-cell facet from hand-off.md; the
## capture analogue NOTE in tests/elle/nested-loop-inner-invariant.lisp points
## here. Both single-loop and nested-loop shapes are covered.

## ── 1. single loop: mutable captured local, closure called in place ──────
## Minimal repro. `s` is `def @s` (mutable → env cell), captured by `cl`,
## `cl` called and dropped within the iteration. With the bug the box was
## freed after iteration 1 and iteration 2 faulted.
(defn single []
  (def @acc 0)
  (def @i 0)
  (while (%lt i 3)
    (def @s @[10 20 30])
    (let [cl (fn [] (get s 0))]
      (assign acc (+ acc (cl))))
    (assign i (%add i 1)))
  acc)
(assert (= (single) 30)
        (concat "single-loop capture cell: expected 30, got " (string (single))))
(println "  1. single-loop mutable captured local survived: ok")

## ── 2. nested loops: binding bound BETWEEN the two loops (cap2.lisp) ─────
## `s` is bound inside the OUTER loop, captured by a lambda built in the
## INNER loop. The box is alloc'd once per activation but `s` is re-bound
## each outer iteration; the in-inner-loop release fired (outer×inner) times.
(defn nested []
  (def @oi 0)
  (def @acc 0)
  (while (%lt oi 2)
    (def @s @[10 20 30])
    (def @ii 0)
    (while (%lt ii 3)
      (let [cl (fn [] (get s 0))]
        (assign acc (+ acc (cl))))
      (assign ii (%add ii 1)))
    (assign oi (%add oi 1)))
  acc)  ## 2 outer × 3 inner × (get s 0)=10  =  60
(assert (= (nested) 60)
        (concat "nested-loop capture cell: expected 60, got " (string (nested))))
(println "  2. nested-loop capture cell (cap2 shape) survived: ok")

## ── 3. closure reads the CURRENT iteration's content ────────────────────
## The box is reused across iterations (one env cell), so the closure must
## read whatever `s` was last bound to in its own iteration. Vary the value
## per iteration to prove the content tracks the re-`def`, not a stale box.
(defn varying []
  (def @acc 0)
  (def @i 1)
  (while (%lt i 4)
    (def @s @[i])
    (let [cl (fn [] (get s 0))]
      (assign acc (+ acc (cl))))  ## adds 1, then 2, then 3
    (assign i (%add i 1)))
  acc)
(assert (= (varying) 6)
        (concat "varying-content capture cell: expected 6, got "
                (string (varying))))
(println "  3. capture cell content tracks per-iteration re-def: ok")

## ── 4. the per-arm compensating release, hoisted past the arm's loop ─────
## The same once-per-activation rule, on the other route to a release.
##
## When a cell's `decref_point` lands inside one arm of a branch, the arms
## that do not hold it get a compensating release of their own — for an env
## cell, after that arm's last use of the binding. Where the arm's use sits
## in a LOOP, that last use is a per-iteration point, so the compensating
## release frees the once-per-activation box on iteration 1 and the next
## iteration reads a recycled cell.
##
## The ingredients: two arms whose bodies both loop over a closure that
## captures the cell, and a reassign between two such branches. Without the
## reassign, or with only one arm using the cell, the release lands outside
## the arms and neither route fires. The counter-factual is section 1's:
## a release that is per-iteration reads a freed cell, and `get` on the
## freed box reports `nil` rather than the array.
##
## `each` is the shape this reaches in ordinary code — it splices its body
## into one arm per sequence type — which is why the direct form is written
## out here and the macro is exercised beside it.
(defn arm-loops [k]
  (def @cell @[1])
  (def @acc 0)
  (if (= k :a)
    (begin
      (def @i 0)
      (while (%lt i 4)
        (let [cl (fn [] (get cell 0))]
          (assign acc (+ acc (cl))))
        (assign i (%add i 1))))
    (begin
      (def @i2 0)
      (while (%lt i2 4)
        (let [cl (fn [] (get cell 0))]
          (assign acc (+ acc (cl))))
        (assign i2 (%add i2 1)))))
  (assign cell @[10])
  (if (= k :a)
    (begin
      (def @j 0)
      (while (%lt j 4)
        (let [cl (fn [] (get cell 0))]
          (assign acc (+ acc (cl))))
        (assign j (%add j 1))))
    (begin
      (def @j2 0)
      (while (%lt j2 4)
        (let [cl (fn [] (get cell 0))]
          (assign acc (+ acc (cl))))
        (assign j2 (%add j2 1)))))
  acc)  ## 4 × 1 + 4 × 10 = 44, whichever arm runs
(assert (= (arm-loops :a) 44)
        (concat "arm-loop capture cell (first arm): expected 44, got "
                (string (arm-loops :a))))
(assert (= (arm-loops :b) 44)
        (concat "arm-loop capture cell (second arm): expected 44, got "
                (string (arm-loops :b))))
(println "  4. per-arm capture-cell release fires once per activation: ok")

## ── 5. the same shape through `each` ─────────────────────────────────────
(defn each-loops [n]
  (def @cell @[1])
  (def @acc 0)
  (each _ in (range 0 n)
    (let [cl (fn [] (get cell 0))]
      (assign acc (+ acc (cl)))))
  (assign cell @[10])
  (each _ in (range 0 n)
    (let [cl (fn [] (get cell 0))]
      (assign acc (+ acc (cl)))))
  acc)
(assert (= (each-loops 4) 44)
        (concat "each-loop capture cell: expected 44, got "
                (string (each-loops 4))))
(println "  5. capture cell survives an `each` body that captures it: ok")

(println "region-capture-cell-loop-uaf: all tests passed")
