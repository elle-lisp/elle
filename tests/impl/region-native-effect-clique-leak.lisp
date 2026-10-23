(elle/epoch 15)
# audited: 2026-10-20
# A native declared to store no argument records no compile-time arg clique, so a loop of calls strands no region.
# docs/impl/region/effects.md
# docs/impl/region/clique.md
#
# An undeclared native is read as a mutual may-store clique: every heap argument
# may be stored into every other, so each heap argument's region takes a
# compile-time `IncrefRegion` at the call site, balanced by the target region's
# free-time cascade ONLY if the store happens. `identical?` stores nothing, so
# under that reading both increfs never balance: each iteration's two
# string-literal regions stay at rc=1 after their `DecrefRegion` drops the
# initial reference — two leaked regions per call.
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
# The only built-in `Mixed` native is `git`, which compiles SPIR-V, so the
# clique loop itself is pinned at compile time by
# `effect_mixed_call_pairs_arguments_not_one_arguments_regions`
# (src/hir/region/infer/tests/effects.rs). Both natives below declare `Opaque`
# and take a single argument: these rows pin that one argument with two source
# regions strands nothing at run time.

(defn two-region-arg [n]
  (let [k (if (%lt n 0)
            (fiber/new (fn [] 1) |:yield|)
            (fiber/new (fn [] 2) |:yield|))]
    (fiber/child k)))

(defn two-region-read [n]
  (let [k (if (%lt n 0) (list 1 2) (list 3 4))]
    (first k)))

(defn churn-two-region [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (two-region-arg i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn churn-two-region-read [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (two-region-read i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(assert (nil? (two-region-arg 1))
        "the branch-valued argument reaches fiber/child")
(assert (nil? (two-region-arg -1)) "the other arm reaches the same call")
(assert (= (two-region-read 1) 3)
        "the branch-valued argument reads back through the sequence read")
(assert (= (two-region-read -1) 1)
        "the other arm reads back through the same call")

(let [t100 (churn-two-region 100)
      t1000 (churn-two-region 1000)
      r100 (churn-two-region-read 100)
      r1000 (churn-two-region-read 1000)]
  (assert (%lt t100 20)
          (string "one argument's own regions were paired at n=100: delta=" t100))
  (assert (%lt t1000 20)
          (string "one argument's own regions were paired at n=1000: delta="
                  t1000))
  (assert (%lt r100 20)
          (string "a sequence read over a two-region argument leaked at n=100: "
                  "delta=" r100))
  (assert (%lt r1000 20)
          (string "a sequence read over a two-region argument leaked at n=1000: "
                  "delta=" r1000)))
