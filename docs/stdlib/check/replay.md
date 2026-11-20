# Replaying a case

<!-- audited: 2026-11-20 -->

`check:run` tests the one case that the bits or the choices of an earlier run make, and shrinks it when it fails.

Given `:bits b`, [check:run](../check.md) tests the one case a replay of `b`
makes, and shrinks it when it fails. It runs no enumeration and no random
case, so `:depth`, `:trials`, `:seed` and `:skips` change nothing. When that
case skips, the run passes with `:cases` 0 and `:skipped` 1. `b` is an array
of 0 and 1, such as the `:bits` of an earlier run.

Given `:choices cs` in place of `:bits`, `check:run` replays the case whose
draws answer the choices of `cs` in order. A choice at or past its draw's
bound answers 0, and so does each draw past the end of `cs`. The case records
each choice in the bits of its draw's width, so it holds no rejected bits.
`cs` is an array of integers of 0 or more, such as the `:choices` of an
earlier run. A replay of a run's `:choices` makes the value that a replay of
its `:bits` makes.

When `b` or `cs` is not such an array, or both are given, `check:run` raises
`:argument-error`. `check:assert` takes both options, as `check:run` does.

```lisp
(def check ((import "std/check")))
(def g (check:int 0 1000))
(defn below-17 [n] (< n 17))
(def r (check:run g below-17 :bits [1 1 1 1 1 0 1 0 0 0]))
(assert (= [1000 17 1 true] [(r :original) (r :value) (r :cases) (r :minimal)]))
(def again (check:run g below-17 :bits (r :bits)))
(assert (= [17 17] [(again :original) (again :value)]) "a replay of :bits")
(def chosen (check:run g below-17 :choices [1000]))
(assert (= [1000 17] [(chosen :original) (chosen :value)]) "a replay of :choices")
(assert (get (check:run g below-17 :choices [1001]) :passed) "1001 answers 0")
```

A bound of 5 reads three bits, so the bits 1 1 1 name 7, which the draw
rejects. A replay of the choice the draw answered holds no rejected bits:

```lisp
(def check ((import "std/check")))
(def five (fn [draw] (draw 5)))
(defn below-3 [n] (< n 3))
(def rejected (check:run five below-3 :bits [1 1 1 0 1 1] :shrinks 0))
(def chosen (check:run five below-3 :choices (rejected :choices) :shrinks 0))
(assert (= [3 3] [(rejected :value) (chosen :value)]))
(assert (= [[1 1 1 0 1 1] [0 1 1]] [(rejected :bits) (chosen :bits)]))
```
