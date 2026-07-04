(elle/epoch 12)
# Counterfactual for the alloc-region side of the user-fn arg-clique rule
# (docs/impl/region-effects.md "Native region effects" — the `None`/user-fn case).
#
# An opaque USER-FN call site (callee is not a registered primitive, so the
# solver knows nothing about its store behaviour) historically recorded the
# FULL mutual arg clique — exactly as a Mixed/Unknown NATIVE does. For a
# call-RESULT argument the clique incref is a slot-based no-op (pinned by
# region-userfn-clique-callresult-noleak.lisp). For an ALLOC-region argument
# (a direct heap literal — its static slot IS populated at runtime) the
# slot-based `IncrefRegion` is REAL and never balances: a user fn can only
# store an argument through the runtime-counted mutable-store funnel
# (region-rules.md Rule 5 is statically complete), so no caller-side compile-
# time incref is ever needed. Each call leaked one region per alloc-region
# heap argument — the dominant leak class (leakfiber.lisp's t0c-concat tiers;
# every multi-heap-arg opaque user-fn call).
#
# `f2`/`f3` are opaque user fns that store nothing. Region growth must be
# bounded and must NOT scale with the number of calls or the arg count.
# Counterfactual: RED before the fix (delta ≈ arity per call — 2000 at n=1000
# for f2, 3000 for f3); GREEN (bounded) after the clique is dropped for the
# `None` effect.

(defn f2 [a b]
  nil)
(defn f3 [a b c]
  nil)

(defn f2-delta [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (f2 "x" "y")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn f3-delta [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (f3 "x" "y" "z")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d100 (f2-delta 100)
      d1000 (f2-delta 1000)]
  (assert (and (%lt d100 20) (%lt d1000 20))
          (string "user-fn clique leaks alloc-region args (f2): d100=" d100
                  " d1000=" d1000)))

(let [d100 (f3-delta 100)
      d1000 (f3-delta 1000)]
  (assert (and (%lt d100 20) (%lt d1000 20))
          (string "user-fn clique leaks alloc-region args (f3): d100=" d100
                  " d1000=" d1000)))

(println "region-userfn-clique-noleak: ok")
