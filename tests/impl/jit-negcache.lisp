(elle/epoch 12)
# audited: 2026-09-29
# The JIT submits a function it rejects to the background worker at most once.
# docs/impl/jit.md
#
# A function whose compilation the JIT rejects must be submitted to the
# background worker AT MOST ONCE. Every later call falls through to the
# interpreter directly — re-submitting could only reproduce the identical
# rejection (the LIR is immutable, keyed by bytecode pointer) and is pure
# wasted work.
#
# Counterfactual: with every function compiled on its first call (the suite's
# eager profile), EVERY call is "hot". Without the negative cache, each call to
# an un-jit'able function re-submits it and saturates the JIT worker: the stdlib
# `-` and `/` build rest-arg closures, so a hot loop over them recompiles
# thousands of times.
#
# `returns-closure` is un-jit'able for the same reason: its body emits
# MakeClosure. We call it far more times than any sane submission bound,
# then assert via `(jit/rejections)` that its `:attempts` stayed at 1.
#
# Under the default policy the function turns hot on its tenth call, and the
# same bound holds. With the JIT off, `(jit/rejections)` is empty and the loop
# asserts nothing.

(defn returns-closure [x]
  (fn [] x))

# The test must be deterministic: the resubmission storm only appears once a
# rejection has been *recorded* and the function is still being called. In a
# tight loop the main thread can outrun the background worker, so we force the
# rejection to land between calls by draining after each call. `(jit/rejections)`
# drains pending background compilations. Without the negative cache the next
# call re-submits (the pending mark is cleared and nothing consults the
# rejections), so `:attempts` climbs once per iteration. A slow per-request loop
# gives the worker the same gap on every iteration.
(def @i 0)
(while (< i 200)
  (returns-closure i)
  (jit/rejections)
  (assign i (+ i 1)))

# Each rejected function must have been submitted at most once. We allow a
# tiny slack (2) for any benign race between the first submit and the
# rejection being recorded; without the cache the storm produces ~200 here, so the
# bound separates the two regimes cleanly.
(each r (jit/rejections)
  (assert (<= r:attempts 2)
          (string "JIT re-submitted un-jit'able fn '" r:name "' " r:attempts
                  " times across " r:calls
                  " calls (negative-cache regression: docs/impl/jit.md)")))

(println "ok: jit negative cache holds")
