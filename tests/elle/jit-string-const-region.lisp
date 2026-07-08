(elle/epoch 12)
## jit/string-const-region — a forced-JIT string literal is an ORDINARY
## allocation in a reclaimable region.
##
## A string literal lowers to a `MaterializeConst` (docs/impl/region/model.md §
## "Constants lower as ordinary allocations"). The JIT bakes no pointer: it emits
## an alloc-helper call (`elle_jit_make_string`, bracketed by push/pop_alloc_region
## like List/MakeArrayMut) that materializes a FRESH `LString` into the caller's
## per-activation region on every execution, freed at its decref_point.

## Witness: the constant a forced-JIT closure returns lives in a real reclaimable
## region (id >= 2), not an immediate (id 0, no heap).
(def s (compile/run-on :jit (fn [] "jit-const-region-xyz")))
(assert (%ge (arena/region-of s) 2)
        (concat "forced-JIT string constant must be an ordinary allocation in a "
                "real reclaimable region — got region "
                (number->string (arena/region-of s))))

(assert (= s "jit-const-region-xyz")
        "forced-JIT string constant reads back correctly")

(println "jit-string-const-region: ok")
