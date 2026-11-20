(elle/epoch 18)
# audited: 2026-11-20
## Property-based testing: generators, every small case up to a bound, random cases past it, and a shrunk counterexample.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## Loaded via: (def check ((import "std/check")))

(fn []
  (def source ((import "std/check/source")))
  (def gen ((import "std/check/gen")))
  (def shrink ((import "std/check/shrink") :source source :gen gen))
  (def runner ((import "std/check/run") :source source :shrink shrink))

  {:constant gen:constant
   :bool gen:bool
   :int gen:int
   :element gen:element
   :one-of gen:one-of
   :array-of gen:array-of
   :tuple gen:tuple
   :map gen:map
   :bind gen:bind
   :filter gen:filter
   :function gen:function
   :assume gen:assume
   :run runner:run
   :assert runner:assert
   :enumerate runner:enumerate
   :sample runner:sample})
