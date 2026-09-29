(elle/epoch 13)
# audited: 2026-09-29
# A compiled tail loop mints and frees a fresh region per allocation, exactly as the interpreter does.
# docs/impl/region/model.md
#
# The per-execution region model (docs/impl/region/model.md,
# Rule 8) holds on every tier: each allocation EXECUTION mints a fresh physical
# region (`runtime_region_for_alloc_slot`, recorded slot->phys in the
# activation's `activation_region_map`), and the matching `DecrefRegion` frees it
# (`take_runtime_region_for_drop_slot`). A compiled activation pushes its own
# region map, allocates into the per-execution region, and resolves each static
# slot through that map.
#
# `f` is a tail-recursive allocator whose `(%pair i i)` is discarded (its
# `DecrefRegion` fires immediately). Repeated NON-tail calls to `f` make it hot,
# so the default policy compiles it; the measured deep tail loop then runs `f`'s
# alloc + self-tail-call entirely in compiled code (self-tail-call reuses one
# activation across iterations — the per-iteration `DecrefRegion` must clear the
# slot and the next iteration must re-mint, exactly like the interpreter
# trampoline).
#
# The counter-factual: a compiled `DecrefRegion` that used the static SLOT id as
# a physical region frees whatever live region shares that small id, which
# corrupts a list into a cycle and runs out of memory. With the JIT off the
# interpreter keeps the deltas near zero, so the file passes on both tiers.

(defn f (i n)
  (if (%lt i n)
    (begin
      (%pair i i)
      (f (%add i 1) n))
    :done))

# Warmup: many NON-tail calls to `f` drive it past the adaptive hotness
# threshold and give the background JIT compile time to land before measuring.
(defn warm (k)
  (var j 0)
  (while (%lt j k)
    (f 0 50)
    (assign j (%add j 1))))
(warm 3000)

(def r0 (arena/region-count))
(def c0 (arena/count))  # Measured run: `f` is JIT-compiled now — a deep tail loop of allocations.
(f 0 50000)
(def dreg (%sub (arena/region-count) r0))
(def dobj (%sub (arena/count) c0))
(println "region-jit-tailloop delta reg=" dreg " obj=" dobj)

# A leak/corruption would grow these by ~50000; bounded means the per-execution
# region is minted AND freed each iteration.
(assert (%lt dreg 50)
        (concat "JIT tail-loop alloc leaks regions, delta="
                (number->string dreg)))
(assert (%lt dobj 50)
        (concat "JIT tail-loop alloc leaks objects, delta="
                (number->string dobj)))
(println "region-jit-tailloop: ok")
