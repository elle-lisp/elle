(elle/epoch 12)
## Multi-form fixture for the whole-file (single-thunk) legacy mode.
##
## A legacy file with several top-level forms is compiled as ONE whole-file thunk
## (compile/whole-module) and run in source order, once per tier, in isolation —
## byte-for-byte a direct `elle FILE` run. This file is THE counter-factual for
## the old per-form slicing, which hoisted every `def`/`var` to run EAGERLY ahead
## of the bare-expression test forms: there, `(def snap (get cell 0))` ran BEFORE
## the `(put cell 0 …)` that writes `cell`, so `snap` captured pre-write garbage
## and the assert failed spuriously. As one thunk the write precedes the read and
## shared mutable state threads through the file exactly as written.
(def base 41)
(def cell @[0])
(put cell 0 (+ base 1))  ## bare expr: writes cell, using the shared def `base`
(def snap (get cell 0))  ## def: reads — MUST see the write on the line above
(assert (= snap 42) "ordered read-after-write")
(assert (= (get cell 0) 42) "shared mutable state across forms")
