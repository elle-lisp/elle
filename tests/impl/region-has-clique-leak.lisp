(elle/epoch 12)
# Counterfactual for the arg-clique leak on a read-only TRAIT DISPATCHER
# (docs/impl/region/effects.md "Native region effects: declared, not guessed",
# the `Opaque` variant).
#
# `has?` resolves its work through the value's trait table, so its RESULT is
# unbounded — `with-traits` may replace `:Collection` with a user closure
# returning anything, and neither `Immediate` nor `Fresh` holds on every path.
# Its STORE side is bounded regardless: the built-in method reads and returns a
# bool, and a user closure is ordinary Elle code, which stores only through the
# runtime-counted mutable-store funnel.
#
# Two properties, two answers — unbounded result, no store — so `has?` declares
# `RegionEffect::Opaque` and the solver emits NO arg-clique edges for it.
# Declaring `Mixed` instead conflates them and buys the full mutual clique: a
# compile-time `IncrefRegion` per heap-argument pair, balanced only by the store
# target's free-time cascade, which never runs because no store happens. With
# two heap arguments that is two leaked regions per call.
#
# The arguments must be direct heap literals, not call results: a call-result
# placeholder's clique incref takes the value-based emission path
# (region-native-clique-callresult-uaf.lisp), so only direct literals pin the
# static-slot `IncrefRegion` route this leak class lives on. An immediate
# argument (`(has? "hello" 3)`) contributes no region and so no pair at all —
# both arguments must be heap for the clique to have an edge to emit.

(defn churn-string [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (has? "hello" "ell")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn churn-set [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (has? |"a" "b"| "a")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# The result is still correct — the declaration changes accounting, not the
# answer, and a probe that stopped exercising the dispatch would measure 0 for
# the wrong reason.
(assert (has? "hello" "ell") "has? substring")
(assert (has? |"a" "b"| "a") "has? set membership")

(let [s100 (churn-string 100)
      s1000 (churn-string 1000)
      t100 (churn-set 100)
      t1000 (churn-set 1000)]
  (assert (%lt s100 20)
          (string "has? string arg-clique region leak at n=100: delta=" s100))
  (assert (%lt s1000 20)
          (string "has? string arg-clique region leak at n=1000: delta=" s1000))
  (assert (%lt t100 20)
          (string "has? set arg-clique region leak at n=100: delta=" t100))
  (assert (%lt t1000 20)
          (string "has? set arg-clique region leak at n=1000: delta=" t1000)))
