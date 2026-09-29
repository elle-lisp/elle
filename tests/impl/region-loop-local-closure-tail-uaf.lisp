(elle/epoch 12)
# audited: 2026-09-23
# A closure created in a loop, whose tail is a fresh allocation, never frees the value it returns.
# docs/impl/region/mechanism.md
#
# THE TRAP. `try_inline_call` re-walks an inlinable callee's body to collect
# cross-region edges at the call site (src/hir/region/infer/walk/inline.rs). The
# re-walk visits the closure body's nodes a second time, in the caller's
# context. `alloc_here` (src/hir/region/infer.rs) therefore reuses the region the
# structural walk recorded while `inline_depth > 0`. An `alloc_here` that mints
# afresh on the re-walk overwrites `alloc_region[body-node]`, and escape's return
# frontier, projected through that map, stops matching the lowerer's view. The
# closure body then releases, as a discarded result,
# the value it is about to return, and the caller's release reads freed memory.
# The SAME closure hoisted OUTSIDE the loop never reaches the re-walk in a
# discarding context, which is why the shape needs the loop.
#
# A UAF, not a leak: the plain VM crashes at the closure's own release. Each
# driver below also bounds the per-iteration growth, so the caller's release of
# the result must reclaim it.

# Absolute small-delta bound (NOT a ratio): a reclaimed loop here drives the
# per-iteration delta to or under zero (warmup frees outweigh the transient
# value), and a ratio of negatives misfires. A genuine 1/iter leak grows d10k
# toward ~n, far past the bound.
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

# (c) the caller binds the result and reads it. The fault is the closure freeing
# its own return value, so it does not depend on the caller discarding it.
(defn drive-read [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [g (fn [] {:a i})]
      (let [r (g)]
        (assert (= (get r :a) i) "the returned struct is live")))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

# warmup, then measure at two scales.
(drive-pair 200)
(drive-struct 200)
(drive-read 200)

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

(let [d100 (drive-read 100)
      d10k (drive-read 10000)]
  (assert (small? d100 d10k)
          (string "loop-local closure tail bound-and-read UAF/leak: d100=" d100
                  " d10k=" d10k)))

(println "region-loop-local-closure-tail-uaf: ok")
