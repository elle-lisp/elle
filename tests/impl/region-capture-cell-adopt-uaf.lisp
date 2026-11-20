(elle/epoch 15)
# audited: 2026-11-19
# A captured binding outlives each release and read through it when the closure that captured it dies first.
# docs/impl/region/adopt.md
# docs/impl/region/borrow.md
#
# The sidecar arms guardfree, so a read of a freed page faults.
#
## Sections 1 and 2: `v` is a top-level binding captured by a closure, so it
## lives in a compiled capture cell. Its init is a call result, so the frame
## releases that result by value through the cell's slot: the release loads
## the cell and unwraps it. The cell is adopted into the closure's region. The
## closure dies at its call, where the routed release also falls, so the
## routed release must run before the closure's drop frees the cell.
##
## Section 3: `u` is a body def, captured by value with no cell, so the closure
## may adopt its array. An opcode read out of `u` before the spawn is consumed
## by `list` after it, so the array must outlive the closure the spawn
## consumed.
##
## The counter-factual: sections 1 and 2 fault when the closure's drop sorts
## first. Two rules keep the order, each enough alone: the cell-route edge in
## src/lir/lower/order.rs, and the tie-break that sorts a value release before
## a plain drop. Section 3 faults when the read's consumer does not raise `u`'s
## tight last use.
##
## A spawn in place of the call in sections 1 and 2 tests no order: the
## spawn's handle holds the closure's region until the join.

## ── 1. the closure dies at its call ────────────────────────────────────────
(begin
  (def v (array 1 2 3))
  (def result
    (let [thunk (fn [] (first v))]
      (thunk)))
  (assert (= result 1) "the closure read the captured array"))
(println "  1. the cell survives the closure's call: ok")

## ── 2. the same shape through a helper whose body the graft inlines ────────
## `call-with`'s body replaces the call, so the closure dies at the call
## inside the grafted body.
(defn call-with [thunk]
  "Call `thunk` and answer what it returned."
  (thunk))
(begin
  (def w (with-traits [1 2 3] {:tag :my-type}))
  (def tagged (call-with (fn [] (traits w))))
  (assert (= (tagged :tag) :my-type) "the closure read the captured traits"))
(println "  2. the cell survives a grafted helper's call: ok")

## ── 3. an element read before the spawn and consumed after it ──────────────
## The `%get` result is a borrow of `u`'s array, so the array lives until
## `list` takes it, past the spawn where the closure dies. In a `let` body,
## `u` is captured by value with no cell.
(let []
  (def u (array (array 1) 3))
  (def pair
    (let [thunk (fn [] (first u))]
      (list (%get u 0) (sys/join (sys/spawn-vm thunk)))))
  (assert (= pair (list [1] [1])) "the read and the worker saw the inner array"))
(println "  3. the array survives a borrow consumed after the spawn: ok")

(println "region-capture-cell-adopt-uaf: all tests passed")
