# Generated functions

<!-- audited: 2026-11-20 -->

A generator of functions whose every call draws its answer from the case, so the shrinker simplifies the answers too.

`(check:function g)` makes a function that takes any arguments and answers a
value of `g` on each call. Making the function draws nothing. Each call draws
its answer with the `draw` of the case that made the function, so the bits of
the case hold every answer, and `:choices` holds the choice of each draw. The
arguments do not decide the answer: two calls with equal arguments draw
twice, and can answer different values.

A call in the property draws after the generator's draws. So
[check:run](../check.md) walks the answers of the calls in its enumeration,
and the shrinker deletes and lowers them as it does any draw.
`check:enumerate` and `check:sample` call no property, so a function they
make answers nothing in its case.

A call raises what `g` raises. When the property makes the call, the case
fails in the property, or skips when `g` skips it. When the generator makes
the call, the case fails in the generator.

```lisp
(def check ((import "std/check")))
(def r (check:run (check:function (check:bool)) (fn [f] (not (and (f) (f))))))
(assert (= [[[[] true] [[] true]]] (r :calls)) "two calls that answered true")
(assert (= [1 1] (r :choices)) "each call drew once")
(assert (= 3 (r :cases)) "the enumeration reaches the second answer")
```

## The calls of a failing case

When `check:run` fails, its result's `:calls` holds the calls of each
function the shrunk case made, in the order the case made them. The calls of
one function are an array of `[args answer]`, in the order they happened, and
`args` is an array of the call's arguments. A function the case made and
never called has an empty array. `:calls` is empty when the case made no
function. When the shrunk case made functions, the message of `check:assert`
names their calls as `:calls` holds them.

A call after its case ends draws on from the source of that case, and
`:calls` leaves it out. Past the bits of a replayed case that source reads
zeros, so the call answers the simplest value of `g`.

```lisp
(def check ((import "std/check")))
(def pair (check:tuple (check:function (check:int 0 100)) (check:int 0 100)))
(defn under-50 [[f x]]
  (< (f x) 50))
(def r (check:run pair under-50))
(assert (= [0 [[[[0] 50]]]] [(get (r :value) 1) (r :calls)]))
(def again (check:run pair under-50 :bits (r :bits)))
(assert (= 0 ((get (again :value) 0) 7)) "a call after the case answers 0")
(assert (= [[[[0] 50]]] (again :calls)) "and :calls leaves it out")
(def [ok? err] (protect (check:assert pair under-50)))
(assert (string/contains? (err :message) "[[[[0] 50]]]"))
```
