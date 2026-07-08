(elle/epoch 12)
## tests/elle/nested-loop-inner-invariant.lisp
##
## Regression: a binding bound BETWEEN two nested loops — INSIDE the outer
## loop body but OUTSIDE the inner loop — and read inside the inner loop
## must survive every inner iteration.
##
## Root cause (closed): the liveness iter-scope last-use extension consulted
## only the OUTERMOST enclosing loop (`iter_scope_stack.first()`). A binding
## bound inside the outer loop is bound-INSIDE that outermost loop, so the
## extension found `bound_outside=false` and never extended its last-use.
## Its decref_point then landed inside the INNER loop body, so the lowerer
## freed the value (decref + nil-stamp the slot) right after its read on the
## inner loop's FIRST iteration. The next inner iteration read nil.
##
## This is exactly how `(each x in row ...)` nested inside `(each row in ...)`
## breaks for INDEXED sequences (arrays/strings/structs): the inner `each`
## expands to `(def @len (length seq)) (while (%lt idx len) ...)`, with `len`
## bound between the two loops. After iteration 0 `len` became nil and the
## bound check `(%lt idx nil)` raised `%lt: ... integer and nil`. The
## list-iterating `each` (which walks `cur`/`pair?`, no `len`) was unaffected,
## which is why list×list nesting worked but array×array did not.
## (Surfaced via lib/portrait.lisp module-portrait, tests/elle/portrait.lisp.)
##
## The fix extends last-use to the OUTERMOST loop the binding is bound
## OUTSIDE of (here: the inner loop), not only the absolute-outermost loop.

## ── 1. nested `each` over arrays (the portrait trigger shape) ────────────
## The inner each's `len` is bound between the two loops. With the bug the
## inner loop raised on the 2nd element of the first row.
(def @flat @[])
(each row in @[@["a" "b"] @["c"] @["d" "e" "f"]]
  (each x in row
    (push flat x)))
(assert (= (length flat) 6)
        (concat "nested each/array: expected 6 elements, got "
                (string (length flat))))
(assert (= (string/join (->array flat) "") "abcdef")
        (concat "nested each/array: expected abcdef, got "
                (string/join (->array flat) "")))
(println "  1. nested each over arrays collected every element: ok")

## ── 2. list (outer) × array (inner): inner `len` still bound between ─────
## Outer list-walk has no `len`; the failing `%lt` was the inner array loop's
## own bound check, proving the clobber is the inner binding, not the outer.
(def @flat2 @[])
(each row in (list @["a" "b"] @["c" "d" "e"])
  (each x in row
    (push flat2 x)))
(assert (= (string/join (->array flat2) "") "abcde")
        (concat "list×array each: expected abcde, got "
                (string/join (->array flat2) "")))
(println "  2. list-outer × array-inner each: ok")

## ── 3. raw while/def, no `each` macro: the minimal mechanism ─────────────
## `il` is a COMPUTED value (so it gets its own region) bound inside the
## outer loop and outside the inner loop, used as the inner loop's bound.
(defn raw-nested []
  (def @oi 0)
  (def @count 0)
  (while (%lt oi 2)
    (def @items @["x" "y" "z"])
    (def @il (length items))  ## bound between the two loops
    (def @ii 0)
    (while (%lt ii il)  ## reads `il` across the inner back-edge
      (assign count (+ count 1))
      (assign ii (%add ii 1)))
    (assign oi (%add oi 1)))
  count)
(assert (= (raw-nested) 6)
        (concat "raw nested while: expected 6, got " (string (raw-nested))))
(println "  3. raw nested while/def with computed inner bound: ok")

## ── 4. triple nesting: a binding bound between loop 1 and loop 2, read in
## loop 3, must outlive loop 2 (the outermost loop it is bound outside of) ─
(def @triple @[])
(each a in @[@[@["p"] @["q"]] @[@["r"]]]
  (each b in a
    (each c in b
      (push triple c))))
(assert (= (string/join (->array triple) "") "pqr")
        (concat "triple nested each: expected pqr, got "
                (string/join (->array triple) "")))
(println "  4. triple-nested each over arrays: ok")

## NOTE: the capture analogue — a `@`-mutable binding captured by a lambda built
## inside a loop — is a DISTINCT defect with a DISTINCT fix, now CLOSED. Its env
## cell (a populate_env cell, not a compiled MakeCaptureCell) is minted once per
## activation, but its DecrefCellRegion was placed at the binding's in-loop last
## use and fired per iteration, freeing the once-allocated cell on iteration 1.
## The fix hoists a cell-release region's decref_point past all enclosing loops
## (regions/analyze.rs `hoist_cell_release_past_loops`); it is NOT this last-use
## extension. It is covered — single-loop AND the bound-between-nested-loops
## (cap2) shape — by tests/elle/region-capture-cell-loop-uaf.lisp, and pinned at
## the solver layer by `env_cell_release_in_loop_hoisted_past_loop`. See
## docs/impl/region/bindings.md "Env cells in loops: release once per
## activation, not per iteration".

(println "nested-loop-inner-invariant: all tests passed")
