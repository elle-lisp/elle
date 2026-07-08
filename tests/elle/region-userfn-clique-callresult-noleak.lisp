(elle/epoch 12)
# Counterfactual for the user-fn side of the hard-edge rule
# (docs/impl/region/effects.md "Hard edges: how a may-store edge is emitted").
#
# An opaque USER-FN call site (callee effect unknowable — here the
# stdlib `put` wrapper, whose `& rest` defeats inlining) records the
# full arg clique. When an argument is a call-result region
# (`(string "v" i)` below), the clique incref must stay the historical
# slot-based NO-OP: the wrapper stores through the mutable-store
# funnel, whose runtime incref already counts the store, and the
# container's free cascade decrefs it exactly once. Making the user-fn
# clique incref value-based (real) adds a retain nothing ever releases
# — one leaked region per stored call-result argument per call (RED at
# the uniform-value-based-emission tree: delta tracked n exactly; the
# leak-suite put/push wrapper tiers went 57 → 65 red).
#
# Only edges at NATIVE call sites with declared uncounted-store effects
# (Stores/Mixed/Unknown) are hard — value-based — edges; that side is
# pinned by region-native-clique-callresult-uaf.lisp. This file pins
# that the restriction holds: user-fn sites stay byte-identical to the
# no-op baseline, so the overwrite-and-cascade accounting balances.
#
# Out of scope (standing enumerated debt, identical at baseline): an
# INLINABLE user-fn wrapper — `(def store! (fn [c v] (put c :x v)))`
# called with a call-result arg — leaks 1/iter via the try_inline_call
# edge shape (the call-result `g`-variant class).

(defn wrapper-put-delta [n]
  (def before (arena/count))
  (def @s @{:x 0})
  (def @i 0)
  (while (%lt i n)
    (put s :x (string "v" i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (wrapper-put-delta 100)
      d10k (wrapper-put-delta 10000)]
  (assert (and (%lt d100 50) (%lt d10k 50))
          (string "stdlib put wrapper call-result arg is not leaked by the clique: d100="
                  d100 " d10k=" d10k)))

(println "region-userfn-clique-callresult-noleak: ok")
