(elle/epoch 12)
## audited: 2026-09-23
## tests/elle/nested-loop-inner-invariant.lisp
##
## A binding bound BETWEEN two nested loops — INSIDE the outer loop body but
## OUTSIDE the inner loop — and read inside the inner loop must survive every
## inner iteration.
##
## The trap: the binding is bound inside the OUTERMOST loop, so an extension
## that asks only about that loop finds it bound inside and never extends its
## last use. Its `decref_point` then lands inside the INNER loop body, the
## lowerer frees the value (decref + nil-stamp the slot) right after its read on
## the inner loop's first iteration, and the next inner iteration reads nil.
## So the liveness pass extends a read's last use to the OUTERMOST loop the
## binding is bound OUTSIDE of — here, the inner loop
## (src/hir/liveness/lastuse/builder.rs).
##
## `(each x in row ...)` nested inside `(each row in ...)` meets this for
## INDEXED sequences (arrays, strings, structs): the inner `each` expands to
## `(def @len (length seq)) (while (%lt idx len) ...)`, with `len` bound between
## the two loops, so a freed `len` makes the bound check raise
## `%lt: ... integer and nil`. The list-walking `each` binds no `len`, which is
## why list×list nesting never met it.

## ── 1. nested `each` over arrays ─────────────────────────────────────────
## The inner each's `len` is bound between the two loops. Freed after its first
## read, it makes the inner loop raise on the 2nd element of the first row.
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
## The outer list walk binds no `len`, so a failing `%lt` here is the inner
## array loop's own bound check: the freed binding is the inner one.
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
## inside a loop — is a different trap with a different answer. Its env cell (a
## `populate_env` cell, not a compiled MakeCaptureCell) is minted once per
## activation, so a release at the binding's in-loop last use would free the
## once-allocated cell on iteration 1. The cell release's `decref_point` moves
## past every enclosing loop instead (`post_loop_placement`,
## src/hir/region/infer/analyze/decref.rs); it is NOT this last-use extension.
## tests/elle/region-capture-cell-loop-uaf.lisp covers it, single-loop and
## bound-between-nested-loops alike, and the solver pins it with
## `env_cell_release_in_loop_hoisted_past_loop`. See docs/impl/region/cells.md
## "Env cells in loops: release once per activation, not per iteration".

(println "nested-loop-inner-invariant: all tests passed")
