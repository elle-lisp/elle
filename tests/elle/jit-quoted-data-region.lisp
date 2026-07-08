(elle/epoch 12)
## jit/quoted-data-region — a forced-JIT quoted COMPOUND literal is an ORDINARY
## allocation in a reclaimable region.
##
## Quoted compound data lowers to a `MaterializeConst` with a recursive template
## (docs/impl/region/model.md § "Constants lower as ordinary allocations"). The
## JIT emits an alloc-helper call (bracketed by push/pop_alloc_region like
## List/MakeArrayMut) that materializes a FRESH structure into the caller's
## per-activation region on every execution, freed at its decref_point — no baked
## constant pointer.

## Witness: a forced-JIT quoted list lives in a real reclaimable region (id >= 2),
## not an immediate (id 0, no heap).
(def s (compile/run-on :jit (fn [] (quote (a b c)))))
(assert (%ge (arena/region-of s) 2)
        (concat "forced-JIT quoted list must be an ordinary allocation in a real "
                "reclaimable region — got region "
                (number->string (arena/region-of s))))
(assert (= s (quote (a b c))) "forced-JIT quoted list reads back correctly")

## nested quoted data through the JIT recursive-materialize path.
(def n (compile/run-on :jit (fn [] (quote (1 "two" [3 4])))))
(assert (%ge (arena/region-of n) 2)
        (concat "forced-JIT nested quoted data must be a real reclaimable region — got region "
                (number->string (arena/region-of n))))
(assert (= n (quote (1 "two" [3 4])))
        "forced-JIT nested quoted data reads back correctly")

(println "jit-quoted-data-region: ok")
