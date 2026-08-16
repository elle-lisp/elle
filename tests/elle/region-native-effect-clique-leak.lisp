(elle/epoch 12)
# Counterfactual for the opaque-call arg-clique leak class
# (docs/impl/region/effects.md "Native region effects: declared, not guessed").
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

# ── One argument, two source regions ────────────────────────────────────
# The clique is over pairs of ARGUMENTS, so a native reached with a single
# heap argument emits no edge — however many regions that one argument's
# value may live in. `k` below has two: a branch's arms are ALTERNATIVES for
# one value, never two values one could store into the other, so no edge
# stands between them. Pairing a flattened region list instead emits a
# compile-time `IncrefRegion` no free cascade balances, stranding one region
# per call with no second argument anywhere in the shape.
#
# `first` declares `RegionEffect::Mixed` — it can hand back a value living
# inside its argument — so this is the clique face, not the no-clique one the
# `identical?` churn above pins.

(defn two-region-arg [n]
  (let [k (if (%lt n 0) (list 1 2) (list 3 4))]
    (first k)))

(defn churn-two-region [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (two-region-arg i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(assert (= (two-region-arg 1) 3)
        "the branch-valued argument reads back through the Mixed native")
(assert (= (two-region-arg -1) 1)
        "the other arm reads back through the same call")

(let [t100 (churn-two-region 100)
      t1000 (churn-two-region 1000)]
  (assert (%lt t100 20)
          (string "one argument's own regions were paired at n=100: delta=" t100))
  (assert (%lt t1000 20)
          (string "one argument's own regions were paired at n=1000: delta="
                  t1000)))
