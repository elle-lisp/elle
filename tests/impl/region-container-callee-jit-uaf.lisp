(elle/epoch 18)
# audited: 2026-11-23
# A compiled caller's closure that re-stores the container it was called through keeps running on a reference of its own.
# docs/impl/region/reads.md
#
# The compiled face of region-container-callee-uaf.lisp. The sidecar compiles
# every function on its first call, synchronously, and arms guardfree.
# `compile/run-on` forces each caller onto the JIT, so the read of the callee
# out of the container is compiled code. The compiled callee then reads its
# capture through its environment pointer.
#
# The counter-factual: the compiled call reads its callee straight out of the
# container and takes no count. The `assign` then frees the running closure
# and its capture, and the read of `x` faults under guardfree.

(when (nil? (vm/config :jit))
  (error {:error :gated :reason "the build compiles nothing"}))

# The sum of the lengths of "cap0" to "cap199".
(def expected 1090)
(def @f nil)

(defn make [i]
  (let [x (string "cap" i)]
    (fn []
      (assign f nil)
      (length x))))

(defn tail-go []
  (f))
(defn body-go []
  (+ 0 (f)))

(defn drive [go]
  (def @total 0)
  (each i in (range 200)
    (assign f (make i))
    (assign total (+ total (compile/run-on :jit go))))
  total)

(assert (= (drive tail-go) expected)
        "a compiled tail call through a container keeps its callee")
(assert (= (drive body-go) expected)
        "a compiled non-tail call through a container keeps its callee")

(println "region-container-callee-jit-uaf: ok")
