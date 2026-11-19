# check

<!-- audited: 2026-11-18 -->

Property-based testing: generators, every small case up to a bound, random cases past it, and a shrunk counterexample.

```lisp
(def check ((import "std/check")))
(def pairs (check:array-of (check:tuple (check:int 0 9) (check:int 0 9)) 8))
(assert (check:assert pairs (fn [xs] (= (sort (sort xs)) (sort xs)))))
```

## A generator draws its choices

A generator is a function of one argument, `draw`. `(draw n)` returns an
integer from 0 to `n` less one, and `n` is between 1 and 2^62. A generator
builds its value from the integers it draws, so the sequence of choices
decides the value. Choice 0 is the simplest answer to every draw, and each
generator below makes its simplest value from it.

| Generator | Value | Simplest |
|-----------|-------|----------|
| `(check:constant v)` | `v` | `v` |
| `(check:bool)` | `false` or `true` | `false` |
| `(check:int lo hi)` | an integer from `lo` to `hi` | the one nearest 0 |
| `(check:element xs)` | an element of the array `xs` | the first |
| `(check:one-of gens)` | a value of one generator in the array `gens` | the first's |
| `(check:array-of g max :min n)` | an array of `n` to `max` values of `g`; `n` is 0 by default | the shortest |
| `(check:tuple g ...)` | an array of one value of each generator | each part's simplest |
| `(check:map f g)` | `(f v)` for a value `v` of `g` | `f` of `g`'s simplest |
| `(check:bind g f)` | a value of the generator `(f v)`, for `v` of `g` | |

`check:int` draws values nearer 0 first, a positive value before its
negative. A generator of your own calls `draw` directly:

```lisp
(def check ((import "std/check")))
(def even (fn [draw] (* 2 (draw 50))))
(assert (= [0 2 4] (check:enumerate even 2)))
(assert (= [0 1 -1 2 -2] (check:enumerate (check:int -5 5) 4)))
```

## Every small case, then random cases

The size of a case is the sum of its choices. `(check:enumerate g depth)`
returns the value of every choice sequence whose size is at most `depth`,
in the lexicographic order of the sequences. A draw past the end of a
sequence answers 0, so the enumeration is finite for a generator that stops
on choice 0.

`(check:sample g :seed s :count n)` returns `n` values drawn at random from
a generator seeded with `s`. The same seed gives the same values on every
run and every build, so a property test stays deterministic.

```lisp
(def check ((import "std/check")))
(def g (check:array-of (check:bool) 3))
(assert (= (check:sample g :seed 7 :count 5) (check:sample g :seed 7 :count 5)))
(assert (= [[] [false] [true] [false false]] (check:enumerate g 2)))
```

## Running a property

`(check:run g prop :depth d :trials t :seed s)` tests the property `prop`
on every case `check:enumerate` gives at depth `d`, then on `t` random cases
from the seed `s`. A case fails when `prop` returns `false` or `nil`, or
raises an error. The defaults are depth 4, 100 trials and seed 0.

`check:run` stops at the first failing case and shrinks it. It runs the
property again on simpler choice sequences, and keeps each one that still
fails. A sequence is simpler when it is shorter, or as long and smaller at
the first choice that differs. The result fails, and no deletion of one
choice and no lowering of one choice by 1 or to 0 gives a failing sequence.

`check:run` returns a struct:

| Key | Holds |
|-----|-------|
| `:passed` | `true` when every case passed |
| `:cases` | how many cases ran |
| `:value` | the shrunk counterexample, when one failed |
| `:choices` | the choice sequence of `:value` |
| `:original` | the first failing case, before shrinking |
| `:error` | what the shrunk case raised, or `nil` when it returned false |

```lisp
(def check ((import "std/check")))
(def r (check:run (check:int 0 1000) (fn [n] (< n 17))))
(assert (not (r :passed)))
(assert (= 17 (r :value)) "shrinking finds the smallest failure")
(def ok (check:run (check:int 0 9) (fn [n] (< n 10)) :trials 5))
(assert (ok :passed))
```

`(check:assert g prop ...)` takes the same arguments and returns `true` when
the property holds. When it fails, it raises `:failed-assertion`, and the
message names the shrunk counterexample and the seed.

```lisp
(def check ((import "std/check")))
(def [ok? err] (protect (check:assert (check:int 0 99) (fn [n] (< n 50)))))
(assert (not ok?))
(assert (= :failed-assertion (err :error)))
```
