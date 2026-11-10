(elle/epoch 15)
# audited: 2026-11-09
# A captured binding's cell outlives the release routed through it when the closure that captured it dies first.
# docs/impl/region/adopt.md
#
# The sidecar arms guardfree, so a read of the freed cell faults.
#
## The trap: `v` is a binding captured by a closure, so it lives in a compiled
## capture cell, and its init is a call result, so the frame releases that
## result by value through the cell's slot: the release loads the cell and
## unwraps it. The closure handed to `sys/spawn-vm` is dead after the spawn.
## In section 3 an element read out of `v` before the spawn is consumed after
## it, so `v`'s array, and the release routed through the cell, outlive the
## closure. With the cell adopted into the closure's region, the closure's
## subtree drop frees the cell first, and the routed release then unwraps a
## freed page. Sections 1 and 2 read nothing after the spawn, so the routed
## release meets the closure's drop there and runs ahead of it.
##
## The counter-factual: the plain VM reads the stale page intact, so the
## program answers correctly unless something recycles the page first. A join
## that parks lets the scheduler do exactly that, on about half its runs.
## Under guardfree the first read faults every time.

## ── 1. the closure dies at the spawn, the join reads through the cell ──────
(begin
  (def v (array 1 2 3))
  (def result
    (let [thunk (fn [] (first v))]
      (sys/join (sys/spawn-vm thunk))))
  (assert (= result 1) "the worker read the captured array"))
(println "  1. the cell survives the closure handed to sys/spawn-vm: ok")

## ── 2. the same shape through a helper whose body the graft inlines ────────
## `on-thread`'s body replaces the call, so the closure's last use is the
## spawn inside the grafted body, exactly as in section 1.
(defn on-thread [thunk]
  "Run `thunk` on a new thread and answer what it returned."
  (sys/join (sys/spawn-vm thunk)))
(begin
  (def w (with-traits [1 2 3] {:tag :my-type}))
  (def tagged (on-thread (fn [] (traits w))))
  (assert (= (tagged :tag) :my-type) "the worker read the captured traits"))
(println "  2. the cell survives a grafted helper's spawn: ok")

## ── 3. an element read before the spawn and consumed after it ──────────────
## The `%get` result is a borrow of `u`'s array, so the array lives until
## `list` takes it, past the spawn where the closure dies. The body is a
## `let`'s: a top-level `begin` splices into the file, where the cell's
## release follows the routed one by another route.
(let []
  (def u (array (array 1) 3))
  (def pair
    (let [thunk (fn [] (first u))]
      (list (%get u 0) (sys/join (sys/spawn-vm thunk)))))
  (assert (= pair (list [1] [1])) "the read and the worker saw the inner array"))
(println "  3. the cell survives a borrow consumed after the spawn: ok")

(println "region-capture-cell-adopt-uaf: all tests passed")
