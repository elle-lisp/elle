(elle/epoch 12)
# Oracle: the JIT must honor the per-execution region model on a hot tail loop
# (Increment 6 — JIT region parity). docs/impl/region-model.md "The per-execution region
# model" + Rule 8 (no leaks).
#
# The interpreter mints a fresh physical region per allocation EXECUTION
# (`runtime_region_for_alloc_slot`, recorded slot->phys in the activation's
# `activation_region_map`) and frees it at the matching `DecrefRegion`
# (`take_runtime_region_for_drop_slot`). The Cranelift JIT never ported this:
#   1. JIT activations never push an `activation_region_map`.
#   2. JIT allocs (`elle_jit_pair`) go into the ambient TLS region, not a fresh
#      per-execution region.
#   3. JIT `DecrefRegion`/`IncrefRegion` use the static SLOT id directly as a
#      physical region (`RuntimeRegion::new(slot)`), so a `DecrefRegion(slot)`
#      frees whatever live runtime region happens to share that small id —
#      a premature free that corrupts a list into a cycle and OOMs.
#
# `f` is a tail-recursive allocator whose `(%pair i i)` is discarded (its
# `DecrefRegion` fires immediately). Repeated NON-tail calls to `f` make it hot,
# so adaptive JIT (the default) compiles it; the measured deep tail loop then
# runs `f`'s alloc + self-tail-call entirely in JIT code (self-tail-call reuses
# one activation across iterations — the per-iteration `DecrefRegion` must clear
# the slot and the next iteration must re-mint, exactly like the interpreter
# trampoline).
#
# RED today: under `--jit=adaptive` this OOMs (the broken JIT explodes — in fact
# stdlib load already OOMs). GREEN once the JIT mints+frees a per-execution
# region per allocation: region/object deltas bounded. Under `--jit=off` the
# interpreter already keeps it bounded (delta ~0), so the file is a valid
# harness on both tiers.

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
