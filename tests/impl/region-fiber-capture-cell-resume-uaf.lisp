(elle/epoch 12)
# audited: 2026-09-23
# A captured mutable local a generator holds across yields is released exactly once.
# docs/impl/region/cells.md
#
# A captured, mutated local held by a generator fiber across a `yield` is a
# per-value CaptureCell (`populate_env`), released ONCE by a `DecrefCellRegion`
# at the binding's last use. The region solver gives such a binding EXACTLY ONE
# cell-release region.
#
# THE TRAP. `try_inline_call` (src/hir/region/infer/walk/inline.rs) re-walks an
# inlined callee's body to collect cross-region edges. When the callee returns a
# fiber whose body has a captured mutable local, the re-walk reaches the nested
# fiber-body lambda's `(var …)` Define a SECOND time. `env_cell_placeholder`
# (src/hir/region/infer/placeholder.rs) is idempotent per binding for that
# reason. Counterfactual: a second mint gives the binding two cell-release
# regions sharing one `decref_point`, which lower to TWO `DecrefCellRegion` for
# one cell. The SECOND resume then double-frees `count`'s region: an abort, or a
# torn CaptureCell read under `--trace=guardfree`.
#
# The `elle test` runner's scheduled per-file thunks are generators with
# captured mutable state, so the corpus runner itself has this shape.

## A generator defined INSIDE a function, so its call site inlines the body and
## re-walks the nested fiber-body lambda. `count` is a mutable local captured AND
## mutated by the nested `inc` closure → a CaptureCell that must live across each
## yield. If its region is freed twice, the count cannot survive the suspends.
(defn make-counter [start]
  (fiber/new (fn []
               (var count start)
               (let [inc (fn [] (assign count (+ count 1)))]
                 (inc)
                 (yield count)  ## suspend holding count's CaptureCell
                 (inc)
                 (yield count)  ## suspend again — cell must still be live
                 (inc)
                 count)) |:yield|))

(def c (make-counter 10))
(assert (= (fiber/resume c) 11) "first yield: count incremented once")
(assert (= (fiber/resume c) 12) "second yield: cell survived the first suspend")
(assert (= (fiber/resume c) 13) "final: cell survived the second suspend")

## A second instance proves the per-execution remap is unaffected and the cell
## is freed exactly once per activation (no leak, no double-free).
(def d (make-counter 100))
(assert (= (fiber/resume d) 101) "second counter: independent state")
(assert (= (fiber/resume d) 102) "second counter: cell survived suspend")

(println "region-fiber-capture-cell-resume-uaf: ok")
