(elle/epoch 12)
# Counterfactual for the arg-clique leak on a native that RE-ENTERS THE VM
# (docs/impl/region/effects.md "Native region effects: declared, not guessed",
# the `Opaque` variant).
#
# `vm/query` picks its operation by a runtime string, so its RESULT is unbounded
# — it is minted by whatever the dispatch reached, in neither the call's own
# region nor an argument's. Its STORE side is not: every operation behind the
# gateway copies its argument out (a Rust `String`, a cloned `Syntax`) or reads
# it, and the Elle code some of them re-enter can store only through the
# runtime-counted mutable-store funnel, exactly as an opaque user fn can.
#
# Two properties, two answers — unbounded result, no store — so `vm/query`
# declares `RegionEffect::Opaque` and the solver emits NO arg-clique edges for
# it. Declaring `Mixed` conflates them and buys the full mutual clique: a
# compile-time `IncrefRegion` per heap-argument pair, balanced only by a store
# target's free-time cascade, which never runs because no store happens. The
# call below takes two heap (string) arguments, so that is two leaked regions
# per call.
#
# The obligation this pins is on the DISPATCH, not the primitive: an operation
# added behind the gateway that RETAINS an argument past the call invalidates
# the declaration and must move it back to `Mixed`. This probe goes RED then.

(defn churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (vm/query "doc" "first")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# The answer must still be right — the declaration changes accounting, not
# behaviour, and a probe that stopped exercising the dispatch would measure 0
# for the wrong reason.
(assert (string? (vm/query "doc" "first")) "vm/query :doc returns a string")

(let [d100 (churn 100)
      d1000 (churn 1000)]
  (assert (%lt d100 20)
          (string "vm/query arg-clique region leak at n=100: delta=" d100))
  (assert (%lt d1000 20)
          (string "vm/query arg-clique region leak at n=1000: delta=" d1000)))
