(elle/epoch 15)
# audited: 2026-10-21
# A top-level binding's cell outlives the release routed through it when the closure that captured it dies first.
#
# docs/impl/region/adopt.md
#
# The sidecar arms guardfree, so a read of the freed cell faults.
#
## The trap: `v` is a top-level `def` captured by a closure, so it lives in a
## compiled capture cell. Its init is a call result, so the frame releases that
## result by value through the cell's slot: the release loads the cell and
## unwraps it. The closure handed to `sys/spawn-vm` is dead after the spawn.
## With the cell adopted into the closure's region, the closure's subtree drop
## would free the cell while the join still runs. The routed release would then
## unwrap a freed page.
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

(println "region-capture-cell-adopt-uaf: all tests passed")
