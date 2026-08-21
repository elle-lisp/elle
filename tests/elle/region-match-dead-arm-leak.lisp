(elle/epoch 12)
# Branch compensation reads the ARM STRUCTURE, not the branch's arity
# (docs/impl/region/mechanism.md § "The return frontier is per-path";
# src/hir/regions/compensate.rs).
#
# A region gets ONE `decref_point` — the textually-last of its uses. When that point
# lands inside a branch arm, every path through a DIFFERENT arm reaches the merge
# without freeing it. For a sibling arm that contains no use of the region at all,
# the release belongs at that arm's HEAD: the arm creates no reference, so the
# callee's own is the only one in existence, and arms are mutually exclusive so the
# head release and the `decref_point` release can never both run.
#
# Nothing in that argument counts arms or names a branch kind. Keying the admission
# on the branch KIND instead — `if` yes, `match` no — refused the dominant family: a
# polymorphic `(match (type-of x) …)` routinely reaches an arm that ignores a value
# whose `decref_point` the solver left in a sibling, so the region — and every member
# its free cascade would have reclaimed — was held to fiber teardown, once per call.
# `dead-arm-struct` below is the discriminator between the two readings: it is a
# TWO-armed `match`, so it is bounded under an arity rule and stranded under a kind
# rule.
#
# Both faces are pinned, because the fix must not become an over-free:
#   LEAK face — drive the arm that does NOT use the value; object count bounded.
#   UAF face  — drive the arm that DOES use it (and, for a returned value, the arm
#               that hands it over) and READ the result; the reference must still be
#               live. Run under `--trace=guardfree` by `region_match_dead_arm_uaf`.
#
# Controls bracket the diagnosis: the same shape written with `if` was already
# compensated, and the arm that owns the `decref_point` was never the leaking one.

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# ── subjects ──────────────────────────────────────────────────────
# `v` is allocated BEFORE the match (live-in on every arm) and used in exactly one
# arm, so its `decref_point` sits there. Every other arm is a DEAD sibling arm.
(defn dead-arm (t)
  (let [v (list 1 2 3)]
    (match t
      :use (length v)
      :skip 0
      _ -1)))

# The dead arm sits FIRST, so the leaking path is not an artifact of arm order.
(defn dead-arm-first (t)
  (let [v (list 1 2 3)]
    (match t
      :skip 0
      :use (length v)
      _ -1)))

# A struct rather than a list: one object instead of three, so the per-call cost
# tracks the region's membership rather than a fixed constant.
(defn dead-arm-struct (t)
  (let [v {:a 1 :b 2}]
    (match t
      :use (get v :a)
      _ -1)))

# The two-armed `if` written the same way, holding everything but the branch KIND
# fixed: same local, same arity, same dead arm.
(defn dead-arm-if (t)
  (let [v (list 1 2 3)]
    (if (%eq t :use) (length v) -1)))

# The RETURN-ESCAPING shape: one arm hands `v` to the caller, the dead arm hands it
# nothing. Head compensation is admitted past the return frontier precisely because
# no mint fired on this path.
(defn dead-arm-return (t)
  (let [v (list 1 2 3)]
    (match t
      :take v
      :skip 0
      _ -1)))

# ── controls: bounded already ─────────────────────────────────────
(def c-if (measure (fn () (dead-arm-if :other)) 200 2000))
(def c-used (measure (fn () (dead-arm :use)) 200 2000))
(def c-carried (measure (fn () (dead-arm-return :take)) 200 2000))
(assert (%lt c-if 200)
        (concat "control: the two-armed `if` dead arm strands the local, delta="
                (number->string c-if)))
(assert (%lt c-used 200)
        (concat "control: the arm holding the decref_point strands the local, delta="
                (number->string c-used)))
(assert (%lt c-carried 200)
        (concat "control: the arm that DOES return the value strands it, delta="
                (number->string c-carried)))

# ── leak face: a `match` arm with no use of the value ─────────────
(def w-skip (measure (fn () (dead-arm :skip)) 200 2000))
(def w-wild (measure (fn () (dead-arm :other)) 200 2000))
(def w-first (measure (fn () (dead-arm-first :skip)) 200 2000))
(def w-struct (measure (fn () (dead-arm-struct :other)) 200 2000))
(def w-ret (measure (fn () (dead-arm-return :skip)) 200 2000))
(println "region-match-dead-arm-leak deltas over 2000 iters:")
(println "  named dead arm:            " w-skip)
(println "  wildcard dead arm:         " w-wild)
(println "  dead arm first in source:  " w-first)
(println "  struct value, dead arm:    " w-struct)
(println "  return-escaping, dead arm: " w-ret)
(assert (%lt w-skip 200)
        (concat "a named `match` arm with no use of the local strands its region, "
                "delta=" (number->string w-skip)))
(assert (%lt w-wild 200)
        (concat "the wildcard `match` arm strands the local's region, delta="
                (number->string w-wild)))
(assert (%lt w-first 200)
        (concat "a dead `match` arm preceding the used one strands the local, "
                "delta=" (number->string w-first)))
(assert (%lt w-struct 200)
        (concat "a dead `match` arm strands a struct local's region, delta="
                (number->string w-struct)))
(assert (%lt w-ret 200)
        (concat "the dead arm of a return-escaping value strands it — no mint "
                "fired on this path, delta=" (number->string w-ret)))

# ── UAF face: the value must survive on the arm that uses it ──────
# A head release emitted on the wrong arm frees `v` before that arm reads it. Under
# `--trace=guardfree` the stale deref detonates; the sums catch a silent recycle on
# the plain tiers. Both arms are driven alternately so a misplaced release cannot
# hide behind a single hot path.
# The struct arm reads a FIELD, so its result is untyped — the sums use stdlib `+`
# rather than raw `%add`, which would demand a proof the field read cannot give.
(var seen 0)
(var k 0)
(while (%lt k 2000)
  (assign seen (+ seen (dead-arm :use)))
  (assign seen (+ seen (dead-arm :skip)))
  (assign seen (+ seen (dead-arm-first :use)))
  (assign seen (+ seen (dead-arm-struct :use)))
  (assign k (%add k 1)))
# per iteration: 3 + 0 + 3 + 1
(assert (%eq seen 14000)
        (concat "a value read on its own `match` arm did not survive, sum="
                (number->string seen)))

# The returned value must still reach the caller: the dead arm's release must not
# have consumed the reference the mint handed over on the returning arm.
(var seen2 0)
(var m 0)
(while (%lt m 2000)
  (let [r (dead-arm-return :take)]
    (assign seen2 (%add seen2 (length r))))
  (dead-arm-return :skip)
  (assign m (%add m 1)))
(assert (%eq seen2 6000)
        (concat "the returned value did not survive its caller's use, sum="
                (number->string seen2)))

(println "region-match-dead-arm-leak: ok")
