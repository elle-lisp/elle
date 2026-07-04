(elle/epoch 12)
# Counterfactual for the opaque-call arg-clique leak class
# (docs/impl/region-effects.md "Native region effects: declared, not guessed").
#
# For an opaque native call the solver historically assumed every heap
# argument may be stored into every other — mutual may-store edges, a
# compile-time `IncrefRegion` of each heap argument's region at the call
# site, balanced by the target region's free-time cascade ONLY if the store
# actually happens. `identical?` stores nothing, so both increfs never
# balance: each iteration's two string-literal regions are left at rc=1
# after their `DecrefRegion` drops the initial reference — two leaked
# regions per call (measured at HEAD: delta 200 at n=100, 2000 at n=1000).
#
# `identical?` declares `RegionEffect::Immediate` (it returns a bool and
# stores no argument), so the solver must emit NO arg-clique edges for it.
# Region-count growth must stay bounded and must NOT scale with the number
# of calls.
#
# The args must be direct heap literals, not call results: a call-result
# placeholder's clique incref takes a different emission path (value-based
# LoadLocal + IncrefValueRegion — see
# region-native-clique-callresult-uaf.lisp), so only direct literals pin
# the static-slot `IncrefRegion` route this leak class lives on.

(defn churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (identical? "a" "b")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d100 (churn 100)
      d1000 (churn 1000)]
  (assert (%lt d100 20)
          (string "native arg-clique region leak at n=100: delta=" d100))
  (assert (%lt d1000 20)
          (string "native arg-clique region leak at n=1000: delta=" d1000)))
