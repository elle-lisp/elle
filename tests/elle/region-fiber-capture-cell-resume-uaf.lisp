(elle/epoch 12)
## tests/elle/region-fiber-capture-cell-resume-uaf.lisp
##
## docs/impl/region/model.md Rule 4/5 + "Two id-spaces": a captured, mutated local held by
## a generator fiber across a `yield` is materialized as a per-value CaptureCell
## (`populate_env`), released ONCE by a `DecrefCellRegion` at the binding's last
## use. The region solver must give such a binding EXACTLY ONE cell-release
## region — one `DecrefCellRegion`. Two would double-free the cell's region.
##
## Root cause (closed): `try_inline_call` (src/hir/regions.rs) re-walks an
## inlined callee's body to discover cross-region edges. When the callee returns
## a fiber whose body has a captured mutable local, that re-walk reached the
## nested fiber-body lambda's `(var …)` Define a SECOND time and
## `env_cell_placeholder` minted a SECOND cell-release region for the SAME
## binding. Two cell-release regions sharing one `decref_point` lowered to TWO
## `DecrefCellRegion` for one cell — a double-free of the CaptureCell's region on
## resume. It surfaces as a `DecrefRegion(N) but region was never
## alloc_in_region'd (or already freed)` phantom/double-free abort, or a torn
## CaptureCell read under `--trace=guardfree`.
##
## This is the minimized, harness-free witness of the `elle test` corpus abort:
## the runner's scheduled per-file thunks are generators with captured mutable
## state, and `concat`/array churn between suspend and resume recycled the
## double-freed region id, turning the latent double-free into a SIGSEGV.
##
## RED before the fix: `(make-counter …)` is inlined at its call site, the fiber
## body's captured `count` gets two `DecrefCellRegion`s, and the SECOND resume
## double-frees `count`'s region → abort. GREEN once `env_cell_placeholder` is
## idempotent per binding (one cell-release region, one `DecrefCellRegion`).

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
