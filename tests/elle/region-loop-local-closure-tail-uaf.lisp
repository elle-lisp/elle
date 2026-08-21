(elle/epoch 12)
# Counterfactual (RED → UAF): a closure CREATED INSIDE a loop whose body's TAIL
# expression is a FRESH allocation, called and the result discarded, must NOT
# free its own return value. The minimal form of leakcall.lisp tier 17a
# (t17-g-variant).
#
# ROOT CAUSE (region solver). `try_inline_call` re-walks an inlinable callee's
# body to discover cross-region edges at the call site (src/hir/regions/walk.rs).
# `alloc_here` keyed every allocation by its HIR id and OVERWROTE the entry, so
# the inlined re-walk of the closure body clobbered `alloc_region[body-node]`
# with a fresh region minted in the caller's (discarding) context — desyncing it
# from `lambda_tail_regions`, which the STRUCTURAL walk recorded for the original
# region. Under the move convention `lower_return` suppresses the return-mint for
# the (now stale) tail region while the lowerer emits a discarded-result
# `DecrefValueRegion` INSIDE the closure body for the clobbered region: the
# closure frees the value it is about to return, and the caller's discard-release
# then derefs freed memory (the stale-region-deref panic, regionstore.rs). The
# SAME closure hoisted OUTSIDE the loop is fine — its body region is not visited
# in a discarding context. Fix: `alloc_here` must reuse an existing region during
# an inlined re-walk (inline_depth > 0) rather than clobber it, mirroring the
# `env_cell_placeholder` re-walk idempotency in the same file.
#
# A UAF, not a leak: crashes the plain VM at the closure's own DecrefValueRegion.
# GREEN once the closure no longer frees its tail-returned value, AND the result
# is reclaimed per iteration (bounded) by the caller's discard-release.


# Absolute small-delta bound (NOT the ratio-based `bounded?`): a reclaimed loop
# here drives the per-iteration delta to/under zero (warmup frees outweigh the
# transient pair), and a ratio of negatives misfires (t17 g-variant precedent in
# leakcall.lisp). A genuine 1/iter leak grows d10k toward ~n, far past the bound.
(defn small? [d100 d10k]
  (and (%lt d100 100) (%lt d10k 100)))

# (a) tail expression is a native-call result (%pair).
(defn drive-pair [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [g (fn [] (%pair i i))]
      (g))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

# (b) tail expression is a struct literal — proves the trigger is the
# tail-position fresh allocation, not %pair specifically.
(defn drive-struct [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [g (fn [] {:a i})]
      (g))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

# NOTE on the discarded-vs-used dimension. The UAF is the closure freeing its
# own return value, so it fires whether the caller discards the result or binds
# and reads it (verified: a `(let [r (g)] (get r :a))` driver also crashed pre-
# fix). The DISCARDED form above is the canonical tier-17a shape and reclaims
# cleanly on BOTH the VM and the eager-JIT tiers, so it is the cross-tier pin.
# The bound-and-read form is NOT pinned here: post-fix it no longer crashes on
# either tier (the UAF is gone), but under eager JIT it carries a residual
# 1/iter move-convention OVER-KEEP (a leak, the safe direction — never a UAF)
# that the VM reclaims. That tier-divergent over-keep is orthogonal move-leak
# accounting (orthogonal move-leak work), not this UAF, and pinning it here would
# couple this regression to unrelated work.

# warmup, then measure at two scales.
(drive-pair 200)
(drive-struct 200)

(let [d100 (drive-pair 100)
      d10k (drive-pair 10000)]
  (assert (small? d100 d10k)
          (string "loop-local closure tail %pair UAF/leak: d100=" d100 " d10k="
                  d10k)))

(let [d100 (drive-struct 100)
      d10k (drive-struct 10000)]
  (assert (small? d100 d10k)
          (string "loop-local closure tail struct UAF/leak: d100=" d100 " d10k="
                  d10k)))

(println "region-loop-local-closure-tail-uaf: ok")
