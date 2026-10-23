(elle/epoch 15)
# audited: 2026-10-22
# A kill lands inside compiled code: a silent loop and a loop that may raise, each compiled.
# docs/impl/jit-fuel.md
#
# The sidecar makes the JIT eager and compiles on the VM thread, and a worker
# boots under the same policy. Each worker warms its loop, checks that the JIT
# compiled it, tells the spawner it is about to enter the loop, and runs it
# with a bound no loop reaches. The spawner kills the worker once told. The
# kill can land at the call that enters the loop, in the microseconds between
# the send and the entry; it cannot land anywhere that makes a check-less
# compiled charge pass.
#
# The counter-factual: a compiled silent charge with no check spins until the
# process is killed, and the join never returns.

(defn spin [n]
  (numeric!)
  (def @i 0)
  (while (%lt i n) (assign i (%add i 1)))
  i)

(defn noisy [n]
  (def @i 0)
  (while (< i n) (assign i (+ i 1)))
  i)

(def no-end 9223372036854775807)

(defn kill-once-ready [spawn]
  "Spawn a worker through `spawn` with a sender it signals before its loop,
   kill the worker once it has signalled, and answer the join's error."
  (let [[tx rx] (chan)
        h (spawn tx)]
    (chan/select @[rx])
    (sys/kill h)
    (let [[ok? err] (protect (sys/join h))]
      (assert (not ok?) "a killed worker answers no value")
      err)))

## The silent loop, compiled.
(let [err (kill-once-ready (fn [tx]
                             (sys/spawn (fn []
                                          (spin 3)
                                          (jit/rejections)
                                          (if (jit? spin)
                                            nil
                                            (error {:error :not-compiled}))
                                          (chan/send tx :ready)
                                          (spin no-end)))))]
  (assert (= (get err :error) :killed)
          "silent: the kill landed in compiled code"))

## The loop that may raise, compiled.
(let [err (kill-once-ready (fn [tx]
                             (sys/spawn (fn []
                                          (noisy 3)
                                          (jit/rejections)
                                          (if (jit? noisy)
                                            nil
                                            (error {:error :not-compiled}))
                                          (chan/send tx :ready)
                                          (noisy no-end)))))]
  (assert (= (get err :error) :killed)
          "raising: the kill landed in compiled code"))

(println "kill-jit: ok")
