(elle/epoch 12)
# A captured heap value handed to an os/spawn worker must survive the worker's
# own region churn and come back intact across the join.
#
# Mechanism (src/primitives/concurrency.rs): a spawn worker reconstructs the
# closure + captures into a single `recv_region` on its own heap, runs the body,
# serializes the result into a SendBundle, then `decref_region(recv_region)` to
# free everything before exiting. If the body's allocation churn frees+recycles
# a region a still-live capture points into — or the result still references
# recv_region after the copy-out — the region-generation guard faults on the
# worker thread (caught by catch_unwind → the join returns [:failed ...], so the
# equality assert below goes RED). Companion to spawn-config-region.lisp, which
# pins the SIG_QUERY-ambient facet; this pins the plain-allocation-churn facet.
#
# Light worker (sys/spawn-vm, primitives only) for most iterations so the churn
# is the cost, not init_stdlib; one heavy (sys/spawn) iteration covers the
# stdlib-loaded recv_region as well.

(var ok 0)

# Light workers: capture a heap value, churn the worker heap, return the capture.
(var i 0)
(while (< i 12)
  (let [cap [(* i 7) (* i 11) (* i 13)]
        h (sys/spawn-vm (fn []
                          (var acc @[])
                          (var k 0)
                          (while (< k 64)
                            (assign acc (push acc (pair k k)))
                            (assign acc @[])
                            (assign k (+ k 1)))
                          cap))]
    (assert (= (sys/join h) [(* i 7) (* i 11) (* i 13)])
            "light-worker capture survived allocation churn")
    (assign ok (+ ok 1)))
  (assign i (+ i 1)))

# Heavy worker: same property with the stdlib-loaded recv_region.
(let [cap {:a [1 2 3] :b "held"}
      h (sys/spawn (fn []
                     (var acc @[])
                     (var k 0)
                     (while (< k 64)
                       (assign acc (push acc (pair k k)))
                       (assign acc @[])
                       (assign k (+ k 1)))
                     cap))]
  (let [got (sys/join h)]
    (assert (= (get got :a) [1 2 3])
            "heavy-worker struct capture survived churn")
    (assert (= (get got :b) "held") "heavy-worker string capture survived churn")
    (assign ok (+ ok 1))))

(assert (= ok 13) "every spawned worker returned its capture intact")
(println "region-spawn-recycle: OK")
