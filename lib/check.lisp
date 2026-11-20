(elle/epoch 18)
# audited: 2026-11-20
## Property-based testing: generators, every small case up to a bound, random cases past it, and a shrunk counterexample.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## Loaded via: (def check ((import "std/check")))

(fn []
  (def bits ((import "std/check/bits")))
  (def errors ((import "std/check/errors")))
  (def source ((import "std/check/source") :bits bits :errors errors))
  (def random ((import "std/check/random") :bits bits :source source))
  (def walk ((import "std/check/walk") :bits bits :source source))
  (def gen ((import "std/check/gen") :errors errors :source source))
  (def case ((import "std/check/case")))
  (def shrink ((import "std/check/shrink") :bits bits :source source :case case))
  (def runner
    ((import "std/check/run") :bits bits :errors errors :source source
                              :random random :walk walk :case case
                              :shrink shrink))

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
