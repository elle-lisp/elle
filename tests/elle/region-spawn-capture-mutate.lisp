(elle/epoch 12)
# A mutated, closure-captured LOCAL of an os/spawn worker must survive the
# worker's own region recycling and come back intact across the join.
#
# Mechanism (src/primitives/concurrency.rs): when the spawned closure has a local
# that one of its own nested closures captures, the worker pre-allocates that
# local's capture cell in its `recv_region` (the `capture_cell(.., recv_region)`
# loop over `capture_locals_mask`). Each `(assign box ...)` is an UpdateCapture,
# which routes through the cross-region store funnel
# (`capture_store_with_rebind`, src/value/arena/mutate.rs): storing a body-region
# array into the recv_region cell increfs the body region, so the body's
# allocation churn cannot free the region the live cell points into; overwriting
# decrefs the displaced region. The body returns the captured value through the
# nested closure, and `SendBundle::from_value` deep-copies it out before
# `decref_region(recv_region)` frees the worker's reconstruction region.
#
# If the funnel failed to retain the stored region, the churn would free it while
# the recv_region cell still referenced it: in debug the region-generation guard
# panics at the worker's deref (caught by catch_unwind -> the join returns
# [:failed ...], so the equality assert goes RED); under --trace=guardfree the
# freed page faults on the worker thread (now that the worker arms the oracle).
# Sibling of region-spawn-recycle.lisp (plain churn returning an immutable
# upvalue) and spawn-config-region.lisp (the SIG_QUERY-ambient facet); this pins
# the captured-local mutation facet.

(var ok 0)

# Light workers: a nested closure captures a mutated local; reassign it to fresh
# body-region arrays each iteration, then return it through the capturing closure.
(var i 0)
(while (< i 8)
  (let [h (sys/spawn-vm (fn []
                          (var box @[(* i 2)])
                          (var getb (fn [] box))
                          (var k 0)
                          (while (< k 48)
                            (assign box @[k (* k 3) (* k 5)])
                            (assign k (+ k 1)))
                          (getb)))]
    (assert (= (sys/join h) @[47 141 235])
            "spawn captured-local cell survived churn (light worker)")
    (assign ok (+ ok 1)))
  (assign i (+ i 1)))

# Heavy worker (sys/spawn, runs init_stdlib): same property, stdlib-loaded
# recv_region, with a captured @struct accumulator mutated in place.
(let [h (sys/spawn (fn []
                     (var acc @{})
                     (var upd (fn [k v] (put acc k v)))
                     (var k 0)
                     (while (< k 32)
                       (upd k @[k (* k 7)])
                       (assign k (+ k 1)))
                     acc))]
  (let [got (sys/join h)]
    (assert (= (get got 0) @[0 0])
            "heavy-worker captured @struct first survived")
    (assert (= (get got 31) @[31 217])
            "heavy-worker captured @struct last survived")
    (assign ok (+ ok 1))))

(assert (= ok 9) "every spawned worker returned its mutated capture intact")
(println "region-spawn-capture-mutate: OK")
