(elle/epoch 18)
# audited: 2026-11-19
# A value a closure captures by value outlives every read out of it that is consumed after the closure dies.
# docs/impl/region/borrow.md
#
# The sidecar arms guardfree, so a read of the freed value faults.
#
## The trap: `u` is captured by value, so the closure may adopt it, and the
## closure's subtree drop then reclaims it. The closure handed to
## `sys/spawn-vm` is dead after the spawn. An opcode read out of `u` before the
## spawn is consumed by `list` after it. The read's result is a borrow of
## `u`'s container, so the container must live until `list` takes it. With `u`
## adopted, the spawn's drop frees it first, and `list` reads a freed page.
##
## The counter-factual: the read's consumer did not raise `u`'s tight last
## use, so the adopt was admitted. The plain VM reads the stale page intact
## unless something recycles it first. Under guardfree each section faults
## every time. A direct call `(thunk)` in place of the spawn keeps the closure
## alive past the read and never faults.

## ── 1. %get reads an element out of an array ───────────────────────────────
(let [u (array (array 1) 3)]
  (let [thunk (fn [] (first u))]
    (assert (= (list (%get u 0) (sys/join (sys/spawn-vm thunk))) (list [1] [1]))
            "the read and the worker saw the inner array")))
(println "  1. %get's result outlives the closure the spawn consumed: ok")

## ── 2. %first reads the head of a list ─────────────────────────────────────
(let [u (pair (array 1) (pair (array 2) ()))]
  (let [thunk (fn [] (first u))]
    (assert (= (list (%first u) (sys/join (sys/spawn-vm thunk))) (list [1] [1]))
            "the read and the worker saw the head")))
(println "  2. %first's result outlives the closure the spawn consumed: ok")

## ── 3. %rest reads the tail of a list ──────────────────────────────────────
(let [u (pair (array 1) (pair (array 2) ()))]
  (let [thunk (fn [] (first u))]
    (assert (= (list (%rest u) (sys/join (sys/spawn-vm thunk)))
               (list (list [2]) [1]))
            "the read saw the tail, the worker the head")))
(println "  3. %rest's result outlives the closure the spawn consumed: ok")

(println "region-capture-adopt-borrow-uaf: all tests passed")
