(elle/epoch 12)
# audited: 2026-09-29
# A compiled non-tail while loop mints and frees a fresh region per allocation, exactly as the interpreter does.
# docs/impl/region/model.md
#
# The companion of region-jit-tailloop.lisp, which describes the mechanism: this
# one pins the non-tail path. `g`'s `(%pair i i)` is built and discarded each
# `while` iteration, and its `DecrefRegion` fires immediately. Repeated calls to
# `g` make it hot, the default policy compiles it, and the measured loop runs the
# alloc in compiled code. A leak here grows both gauges by ~50000 (Rule 8).

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
