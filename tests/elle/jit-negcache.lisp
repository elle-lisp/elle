(elle/epoch 12)
# JIT negative-cache invariant (docs/impl/jit.md, "Rejection tracking").
#
# A function whose compilation the JIT rejects must be submitted to the
# background worker AT MOST ONCE. Every later call falls through to the
# interpreter directly — re-submitting could only reproduce the identical
# rejection (the LIR is immutable, keyed by bytecode pointer) and is pure
# wasted work.
#
# Counterfactual: under `--jit=eager` the hotness threshold is 0, so EVERY
# call is "hot". Without the negative cache, each call to an un-jit'able
# function re-submits it, saturating the JIT worker. This is what made
# h2-stress-scoped burn ~460s of CPU (the hot stdlib `-`/`/` build rest-arg
# closures → MakeClosure rejection, re-compiled thousands of times).
#
# `returns-closure` is un-jit'able for the same reason: its body emits
# MakeClosure. We call it far more times than any sane submission bound,
# then assert via `(jit/rejections)` that its `:attempts` stayed at 1.
#
# Under `--jit=off` / `--jit=adaptive` builds `(jit/rejections)` is empty
# (no eager storm to guard against), so the loop body asserts nothing and
# the test trivially passes — exactly correct.

(defn returns-closure [x]
  (fn [] x))

# We must witness the bug deterministically: the storm only appears once a
# rejection has been *recorded* and the function is still being called. In a
# tight loop the main thread can outrun the background worker, so we force the
# rejection to land between calls by draining after each call. `(jit/rejections)`
# drains pending background compilations; pre-fix the next call then re-submits
# (jit_pending was cleared but nothing consults jit_rejections), so `:attempts`
# climbs once per iteration. This mirrors h2-stress, where the slow per-request
# loop gave the worker the same gap on every iteration.
(def @i 0)
(while (< i 200)
  (returns-closure i)
  (jit/rejections)
  (assign i (+ i 1)))

# Each rejected function must have been submitted at most once. We allow a
# tiny slack (2) for any benign race between the first submit and the
# rejection being recorded; the pre-fix storm produces ~200 here, so the
# bound separates the two regimes cleanly.
(each r (jit/rejections)
  (assert (<= r:attempts 2)
          (string "JIT re-submitted un-jit'able fn '" r:name "' " r:attempts
                  " times across " r:calls
                  " calls (negative-cache regression: docs/impl/jit.md)")))

(println "ok: jit negative cache holds")
