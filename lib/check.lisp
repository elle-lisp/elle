(elle/epoch 15)
# audited: 2026-11-18
## Property-based testing: generators, every small case up to a bound, random cases past it, and a shrunk counterexample.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## Loaded via: (def check ((import "std/check")))

(fn []
  (def source ((import "std/check/source")))
  (def gen ((import "std/check/gen")))
  (def runner ((import "std/check/run") :source source))

  {:constant gen:constant
   :bool gen:bool
   :int gen:int
   :element gen:element
   :one-of gen:one-of
   :array-of gen:array-of
   :tuple gen:tuple
   :map gen:map
   :bind gen:bind
   :run runner:run
   :assert runner:assert
   :enumerate runner:enumerate
   :sample runner:sample})
