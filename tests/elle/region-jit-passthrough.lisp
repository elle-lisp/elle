(elle/epoch 12)
# ── Region: JIT native pass-through must retain the result region ──────
#
# A native "pass-through" primitive (`first`, `rest`, `get`) returns a value
# that lives in one of its arguments' regions, not in a freshly minted one.
# The calling convention hands the caller one owning reference to that region
# (the "pass-through retain"), which the caller's `ReleaseValueRegion` at the
# result's free_at consumes. The interpreter does this retain inline in
# `call_inner`; the JIT must do the identical retain in `elle_jit_call`.
#
# Counter-factual: with the JIT retain missing, a recursive list builder that
# is JIT-compiled (hot under the adaptive tier) under-counts each element's
# region. `(%pair (first xs) acc)` statically increfs the element region for
# the new cons, and the call-result `ReleaseValueRegion` decrefs it — but with
# no native retain the net is zero, so the element region is freed while the
# cons still references it. Reading the elements back then derefs freed memory.
# Under `--trace=guardfree` (freed pages mprotected, never recycled) this
# faults at the exact deref; in a plain run the recycled slot corrupts the
# element. `build`/`collect` in `src/core.lisp` (append/concat) are exactly
# this shape, so the bug also crashes stdlib load in adaptive mode.

# A list builder that mirrors core's append-list `collect`: each step does a
# native pass-through (`first`/`rest`) and stores the element into a fresh
# cons. Recursion + repeated calls make it hot, so the adaptive tier JIT-
# compiles it; the JIT-compiled body must keep the element regions alive.
(def reverse-onto
  (fn [xs acc]
    (if (empty? xs)
      acc
      (reverse-onto (rest xs) (%pair (first xs) acc)))))

(def sum-list
  (fn [xs acc]
    (if (empty? xs)
      acc
      (sum-list (rest xs) (+ acc (first xs))))))

# Drive enough iterations to cross the JIT hotness threshold, then read every
# element back out of the rebuilt list. If any element region was freed early,
# the read derefs freed memory (guardfree: hard fault; plain: wrong value).
(def @i 0)
(while (< i 60)
  (let [src (list 1 2 3 4 5 6 7 8)
        rebuilt (reverse-onto src ())
        total (sum-list rebuilt 0)]
    (assert (= total 36)
            "every element survives the JIT-compiled pass-through rebuild"))
  (assign i (+ i 1)))

# The stdlib `append` is the same pattern through core's `build`/`collect`;
# exercise it directly so the regression also covers the real stdlib path.
(def @j 0)
(def @acc ())
(while (< j 40)
  (assign acc (append acc (list j)))
  (assign j (+ j 1)))
(assert (= (length acc) 40) "append accumulates without freeing element regions")
(assert (= (first acc) 0) "append head element survives")
(assert (= (last acc) 39) "append tail element survives")

(println "region-jit-passthrough: ok")
