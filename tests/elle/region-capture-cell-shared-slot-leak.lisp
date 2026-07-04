(elle/epoch 12)
# Counterfactual: a top-level Begin with TWO OR MORE captured defs leaks every
# pre-allocated capture cell but the last (docs/impl/region-model.md, "The per-execution
# region model": one allocation execution per slot between drops).
#
# Mechanism: `lower_begin` pre-allocates one `MakeCaptureCell` per captured
# top-level binding. Emitting them all against the Begin's ONE region slot
# means each `runtime_region_for_alloc_slot` call mints a fresh physical region
# and OVERWRITES the activation frame's slot mapping — so the slot's single
# `DecrefRegion` releases only the last cell's region. Every earlier cell keeps
# its initial (mint) reference forever: one leaked region per extra captured
# def per compile/run. At stdlib scale this was thousands of CaptureCell
# regions plus every closure they pin (the dominant class of the teardown
# residue).
#
# The shape needs (a) >= 2 captured top-level bindings — here A (captured by
# B's inner letrec lambda) and B (captured by D) — and (b) nothing else: D is
# never called, the program value is nil. Each `(eval …)` compiles and runs one
# such module; a leaking compile grows the live region count linearly.

(defn leak-loop [n]
  (var i 0)
  (while (%lt i n)
    (eval '(begin
             (def cap-a (fn [x] x))
             (def cap-b
               (fn [s]
                 (letrec [go (fn [j] cap-a)]
                   (go 0))))
             (def cap-d (fn [x] (cap-b x)))
             nil))
    (assign i (%add i 1))))

# Warm first (one-time compile/intern effects), then measure growth over a
# fixed window: bounded leaves the region count ~flat, the shared-slot leak
# grows >= 1 region per iteration.
(leak-loop 50)
(def before (arena/region-count))
(leak-loop 500)
(def delta (%sub (arena/region-count) before))
(assert (%lt delta 100)
        (string "shared-slot capture cells leak: live region count grew by "
                delta " over 500 evals (expected ~flat)"))

(println "region-capture-cell-shared-slot-leak: ok")
