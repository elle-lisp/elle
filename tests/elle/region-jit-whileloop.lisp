(elle/epoch 12)
# Oracle: the JIT must honor the per-execution region model on a hot NON-tail
# `while` loop (Increment 6 — JIT region parity). Companion to
# region-jit-tailloop.lisp; this pins the non-tail path, proving the leak is the
# whole per-execution model on the JIT, not tail-specific.
#
# See region-jit-tailloop.lisp for the mechanism. `g`'s `(%pair i i)` is built
# and discarded each `while` iteration; its `DecrefRegion` fires immediately, so
# the interpreter keeps it bounded. Repeated calls to `g` make it hot → adaptive
# JIT compiles it → the measured loop runs the alloc in JIT code.
#
# RED today: OOM under `--jit=adaptive` (the broken JIT explodes). GREEN once the
# JIT mints+frees a per-execution region per allocation. docs/impl/region-rules.md Rule 8.

(defn g (n)
  (var i 0)
  (while (%lt i n)
    (%pair i i)
    (assign i (%add i 1)))
  :done)

# Warmup: drive `g` hot and let the background JIT compile land.
(defn warmg (k)
  (var j 0)
  (while (%lt j k)
    (g 50)
    (assign j (%add j 1))))
(warmg 3000)

(def r0 (arena/region-count))
(def c0 (arena/count))  # Measured run: `g` is JIT-compiled now — a long while loop of allocations.
(g 50000)
(def dreg (%sub (arena/region-count) r0))
(def dobj (%sub (arena/count) c0))
(println "region-jit-whileloop delta reg=" dreg " obj=" dobj)

(assert (%lt dreg 50)
        (concat "JIT while-loop alloc leaks regions, delta="
                (number->string dreg)))
(assert (%lt dobj 50)
        (concat "JIT while-loop alloc leaks objects, delta="
                (number->string dobj)))
(println "region-jit-whileloop: ok")
