(elle/epoch 12)
# audited: 2026-09-23
# A value stored into a persistent container in every arm of a type-dispatch match is released on every arm.
# docs/impl/region/compensate.md
#
# The region solver gives a region ONE `decref_point`, the textually-last of its
# uses. When `val` is used in every arm of a match (each arm passes it to a
# different store intrinsic), that single release lands in the LAST arm. A
# scrutinee that selects an EARLIER arm uses `val` and never frees it. Per-arm
# compensation (src/hir/region/infer/compensate.rs) adds the missing release on
# each used sibling arm, after the store whose retain funds it. Without it `val`'s
# region strands once per call, and a loop makes that unbounded RSS.
#
# A LEAK, not a UAF — live-object growth (`arena/count`). Stdlib `put` takes this
# path: its `(match (type-of coll) ...)` stores the value in every arm.

(defn bounded? [d100 d10k limit]
  "True if both deltas are under limit and 10000 is not ~100x 100. A
  non-positive d100 means no per-iteration growth (net reclamation), so the
  leak-scaling check is vacuous — reclaiming MORE than the baseline is bounded."
  (and (%lt d100 limit) (%lt d10k limit)
       (or (%le d100 0) (%lt d10k (* d100 10)))))

# ── subject: the put-shaped match dispatch, value stored in every arm ──────────
# `coll` is a PARAMETER, so its type is not statically known here and the off-type
# arms are not pruned — `val` is live in every arm.
(defn put-dispatch [coll key val]
  (match (type-of coll)
    :array (%put-array coll key val)
    :@struct (%put-struct-mut coll key val)
    _ (%put coll key val)))

(defn drive-dispatch [n]
  (def s @{:data 0})
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (put-dispatch s :data {:v i})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

# ── subject: stdlib `put` directly (variadic + the same match) ─────────────────
(defn drive-put [n]
  (def s @{:data 0})
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (put s :data {:v i})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

# warm caches / one-time allocations before measuring
(drive-dispatch 200)
(drive-put 200)

(let [d100 (drive-dispatch 100)
      d10k (drive-dispatch 10000)]
  (assert (bounded? d100 d10k 50)
          (string "match-dispatch store leaks: d100=" d100 " d10k=" d10k)))

(let [d100 (drive-put 100)
      d10k (drive-put 10000)]
  (assert (bounded? d100 d10k 50)
          (string "stdlib put store leaks: d100=" d100 " d10k=" d10k)))

(println "region-match-dispatch-store-leak: ok")
