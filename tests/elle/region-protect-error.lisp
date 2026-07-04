(elle/epoch 12)
# ── Region: an error captured by `protect` must outlive the resume ────
#
# `protect` runs its body in a child fiber; on error the fiber's `signal`
# holds the error struct, returned to the parent as `[ok? err]`. The error
# is allocated deep in the child (here in a nested `(fn [] …)` call) and
# the parent `ReleaseValueRegion`s the resume result — so without a region
# pin on the fiber's terminal `signal` the error struct is freed before the
# parent reads it (use-after-free; see vm/fiber.rs `retain_signal_region`).
#
# Counter-factual: with that pin removed this reads freed memory — under
# `--trace=guardfree` it faults at the exact deref; in a plain run the
# recycled slot corrupts `(get err :error)`. The loop forces enough region
# churn that the stale slot is reused with a mismatching tag.

# Single capture: the error survives and is readable.
(let [[ok? err] (protect ((fn [] (apply arena/count [:global]))))]
  (assert (not ok?) "protect captures the nested arity-error as failure")
  (assert (= (get err :error) :arity-error)
          "captured error struct is readable after the resume"))

# Looped capture: every iteration must read the correct error, even as the
# protect-result regions are minted and freed repeatedly.
(def @n 0)
(while (< n 200)
  (let [[ok? err] (protect ((fn [] (apply arena/count [:global]))))]
    (assert (not ok?) "looped protect: failure")
    (assert (= (get err :error) :arity-error) "looped protect: error readable"))
  (assign n (+ n 1)))

# The error value also survives being threaded through other bindings
# before it is read (the destructured binding is not its last use).
(let [[ok? err] (protect ((fn [] (apply arena/count [:global]))))
      kind (get err :error)]
  (assert (not ok?) "threaded protect: failure")
  (assert (= kind :arity-error) "threaded protect: error readable via rebinding"))

(println "region-protect-error: ok")
