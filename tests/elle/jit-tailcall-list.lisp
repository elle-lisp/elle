(elle/epoch 7)
# Regression: JIT self-tail-call with list argument must not corrupt
# the list via slab pool rotation.
#
# count-list tail-calls itself with (rest lst). The rest result shares
# structure with the original list. If JIT rotation frees the original
# list's cons cells, (first lst) in the next iteration reads freed memory.

(defn count-list [lst acc]
  (if (empty? lst) acc
    (count-list (rest lst) (+ acc 1))))

# Build a list long enough to trigger JIT compilation (threshold ~10 calls)
(def big-list (range 200))
(def result (count-list big-list 0))
(assert (= result 200) (string "expected 200, got " result))
(println "jit-tailcall-list: ok")
