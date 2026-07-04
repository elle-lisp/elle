(elle/epoch 12)
# Counterfactual (RED before per-arm decref compensation): a value used in EVERY
# arm of a `(match (type-of x) ...)` and then stored into a persistent container
# must be bounded per iteration. It leaks one object per call.
#
# ROOT CAUSE (decref placement, region analysis). The region solver gives a region
# ONE `decref_point` — the textually-last of its uses. When `val` is used in every
# arm of a match (each arm passes it to a different store intrinsic), that single
# decref lands in the LAST arm. For any scrutinee that selects an EARLIER arm, the
# taken arm uses `val` but never frees it (the decref sits on the unreached last
# arm), so `val`'s region is stranded — a per-call leak that a loop makes unbounded
# RSS. The dead-sibling-arm case (`val` used in only one arm) is already covered by
# `src/hir/regions/compensate.rs`; this is the every-arm-uses-it case, closed by
# per-arm decref placement (a release at `val`'s last use within each sibling arm,
# routed through `emit_decrefs_for` so it fires AFTER the arm's store, never before).
#
# A LEAK, not a UAF — live-object growth (`arena/count`). This is exactly the path
# stdlib `put`/`set` take: their `(match (type-of coll) ...)` stores the value in
# every arm. GREEN once the per-call growth is bounded.

(defn bounded? [d100 d10k limit]
  "True if both deltas are under limit and 10000 is not ~100x 100."
  (and (%lt d100 limit) (%lt d10k limit) (or (= d100 0) (%lt d10k (* d100 10)))))

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
