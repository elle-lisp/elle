(elle/epoch 12)
# Counterfactual for the arg-clique leak on the compile gateway
# (docs/impl/region/effects.md "Native region effects: declared, not guessed",
# the `Opaque` variant).
#
# Every `compile/*` gateway takes two heap arguments and stores neither: SOURCE
# and NAME are copied out to Rust `&str`, FORMS are cloned into owned `Syntax`,
# and the thunk each loader runs is compiled FROM the source rather than from
# the argument Values. What re-entering the VM makes unbounded is the RESULT.
#
# Two properties, two answers — unbounded result, no store — so each declares
# `RegionEffect::Opaque` and the solver emits NO arg-clique edges. Declaring
# `Mixed` conflates them and buys the full mutual clique: one compile-time
# `IncrefRegion` per heap-argument pair, balanced only by a store target's
# free-time cascade, which never runs because no store happens. At two heap
# arguments that is two leaked regions per call.
#
# `compile/dumps` is the face that isolates the clique: it renders its artifacts
# into fresh strings and holds nothing else per call, so the clique IS its whole
# per-call growth and it must measure bounded. The module loaders
# (`compile/whole-module`, `compile/barrier-module`,
# `compile/whole-module-syntax`) are not a gauge for this class — each grows by
# three regions per call from its thunk-run result path, and that growth is
# unmoved by the declaration, so a probe on one of them would report the same
# number before and after. `compile/read-forms` is the in-file control: the same
# two string arguments, already declared `Fresh`, and so already at zero, which
# is what proves the two string arguments are not inherently leaky.

(defn churn-dumps [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (compile/dumps "(+ 1 2)" "<clique-probe>")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn churn-read-forms [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (compile/read-forms "(+ 1 2)" "<clique-probe>")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# The answer must still be right — the declaration changes accounting, not
# behaviour, and a probe that stopped exercising the dispatch would measure 0
# for the wrong reason.
(assert (has? (compile/dumps "(+ 1 2)" "<clique-probe>") :hir)
        "compile/dumps renders the :hir artifact")

(let [ctrl (churn-read-forms 100)]
  (assert (%lt ctrl 20)
          (string "compile/read-forms control is not at zero: delta=" ctrl)))

(let [d20 (churn-dumps 20)
      d100 (churn-dumps 100)]
  (assert (%lt d20 20)
          (string "compile/dumps arg-clique region leak at n=20: delta=" d20))
  (assert (%lt d100 20)
          (string "compile/dumps arg-clique region leak at n=100: delta=" d100)))
