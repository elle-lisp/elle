# check

<!-- audited: 2026-11-20 -->

Property-based testing: generators, every small case up to a bound, random cases past it, and a shrunk counterexample.

```lisp
(def check ((import "std/check")))
(def pairs (check:array-of (check:tuple (check:int 0 9) (check:int 0 9)) 8))
(assert (check:assert pairs (fn [xs] (= (sort (sort xs)) (sort xs)))))
```

## A generator draws its choices from bits

A generator is a function of one argument, `draw`. `(draw n)` returns an
integer from 0 to `n` less one, and `n` is between 1 and 2^62. A generator
builds its value from the integers it draws, so the sequence of choices
decides the value. Choice 0 is the simplest answer to every draw, and each
generator below makes its simplest value from it.

A draw reads its choice from a stream of bits. It reads as many bits as `n`
less one needs, the most significant first. When those bits name `n` or more,
the draw rejects them and reads as many bits again. Past the end of the
stream every bit is 0, so every draw ends. The bits a case's draws record,
the rejected bits included, are that case.

`(draw n edges)` names `edges`, an array of choices from 0 to `n` less one.
One random draw in 8 answers one of them, each as often as the others. It
records the bits of that choice as a draw that read them would, so a replay
of the case answers the same choice. The enumeration and a replay ignore
`edges`. A draw that names no edges reads no bits to decide. When `edges` is
not such an array, the draw raises `:argument-error`.

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
| `(check:filter g pred)` | a value `v` of `g` for which `(pred v)` holds | the simplest of `g`'s that `pred` keeps |

`check:int` draws values nearer 0 first, a positive value before its
negative. Its edges are `lo`, `hi`, and 0 when the range holds 0. A
generator of your own calls `draw` directly:

```lisp
(def check ((import "std/check")))
(def even (fn [draw] (* 2 (draw 50))))
(assert (= [0 2 4] (check:enumerate even 2)))
(assert (= [0 1 -1 2 -2] (check:enumerate (check:int -5 5) 4)))
(def top (fn [draw] (draw 1000 [999])))
(assert (= [0 1 2] (check:enumerate top 2)) "the enumeration ignores edges")
(def big (bit/shl 1 40))
(def xs (check:sample (check:int (- big) big) :count 400))
(assert (all? (fn [v] (any? (fn [x] (= x v)) xs)) [(- big) 0 big]))
```

`(draw g lo hi)` returns an array of `lo` to `hi` values of the generator
`g`, where `0 <= lo <= hi`. It draws the length as one choice, then each
element in order. The source records which bits make each element, so the
shrinker can delete a run of them. `check:array-of` makes its arrays this way.
An array built from a count the generator draws itself gives the shrinker no
element to delete.

```lisp
(def check ((import "std/check")))
(def digits (fn [draw] (draw (check:int 0 9) 1 3)))
(assert (= [[0] [1] [2] [0 0] [0 1] [1 0] [0 0 0]] (check:enumerate digits 2)))
```

## Every small case, then random cases

The size of a case is the sum of its choices. `(check:enumerate g depth)`
returns the value of every choice sequence whose size is at most `depth`,
in the lexicographic order of the sequences. A draw past the end of a
sequence answers 0, so the enumeration is finite for a generator that stops
on choice 0.

`(check:sample g :seed s :count n)` returns `n` values of `g` drawn from
random bits, a SplitMix64 stream seeded with `s`. The same seed gives the
same values on every run and every build, so a property test stays
deterministic. The defaults are seed 0 and 100 values.

```lisp
(def check ((import "std/check")))
(def g (check:array-of (check:bool) 3))
(assert (= (check:sample g :seed 7 :count 5) (check:sample g :seed 7 :count 5)))
(assert (= [[] [false] [true] [false false]] (check:enumerate g 2)))
```

## Skipping a case

`(check:assume ok)` returns `true` when `ok` is true. When `ok` is `false` or
`nil`, it skips the case, which then neither passes nor fails. A generator or
a property can call it. `check:filter` calls it on `(pred v)`, so a value
the filter rejects skips the whole case, and the filter draws nothing again.

`check:assume` skips a case by raising `:check-skip`. When a property catches
that error with a `protect` of its own, the case does not skip.

`check:enumerate` leaves a skipped case out. `check:sample` draws until it has
`n` values, and raises `:check-gave-up` when `10 * n` cases skip.

```lisp
(def check ((import "std/check")))
(def odd (check:filter (check:int 0 9) odd?))
(assert (= [1 3 5 7 9] (check:enumerate odd 9)))
(assert (all? odd? (check:sample odd :seed 3 :count 20)))
(def [ok? err] (protect (check:assume (> 1 2))))
(assert (= [false :check-skip] [ok? (err :error)]))
(def pairs (check:tuple (check:int 0 9) (check:int 0 9)))
(assert (check:assert pairs (fn [[a b]] (check:assume (< a b)) (neg? (- a b)))))
```

## Running a property

`(check:run g prop :depth d :trials t :seed s :skips k :shrinks m :bits b)`
tests the property `prop` on every case `check:enumerate` gives at depth `d`,
then on `t` random cases from the seed `s`. A case fails when `prop` returns
`false` or `nil`, or raises an error. A case also fails when the generator
raises, and then the case has no value. The defaults are depth 4, 100 trials
and seed 0.

A skipped case does not count as one of the `t` random cases, so `check:run`
draws random cases until `t` of them pass or one fails. When `k` random
cases skip, `check:run` gives up. The default of `k` is `10 * t`. The
enumeration ends by itself, so its skipped cases do not count towards `k`.

`check:run` stops at the first failing case and shrinks it. It runs the
generator and the property again on simpler bit sequences, the candidates. It
keeps each candidate that fails in the same place as the first failure: in
the generator, or in the property. So a property that fails never shrinks to
a defect in its generator, and a candidate whose case skips is never kept. A
sequence is simpler when it is shorter, or as long and smaller at the first
bit that differs. So rejected bits make a case less simple, and so does a
draw that reads more bits.

| Step | What the shrinker changes |
|------|---------------------------|
| Delete a draw | the bits of one draw |
| Delete a rejection | the bits one draw rejected |
| Delete elements | the bits of a run of consecutive elements of an array that `(draw g lo hi)` made, and that array's length, written that many less; the array keeps at least `lo` elements |
| Swap elements | the bits of two neighboring elements of an array that `(draw g lo hi)` made, each moved to the other's place |
| Lower a choice | the choice of one draw, to a smaller one |

The shrinker runs a candidate only when it is simpler than the case it keeps.
It runs at most `m` candidates, and `m` is 1000 by default. The result fails
in the same place. Unless the cap stopped the shrinker, no one step gives a
simpler sequence that fails there, where a lowered choice is the choice less 1
or 0.

Given `:bits b`, `check:run` tests the one case a replay of `b` makes, and
shrinks it when it fails. It runs no enumeration and no random case, so `d`,
`t`, `s` and `k` change nothing. When that case skips, the run passes with
`:cases` 0 and `:skipped` 1. `b` is an array of 0 and 1, such as the `:bits`
of an earlier run. When `b` is not such an array, or `m` is not an integer of
0 or more, `check:run` raises `:argument-error`.

`check:run` returns a struct:

| Key | Holds |
|-----|-------|
| `:passed` | `true` when every case that did not skip passed |
| `:cases` | how many cases passed or failed |
| `:skipped` | how many cases skipped |
| `:failed-in` | `:generator` or `:property`: where the shrunk case failed |
| `:value` | the shrunk counterexample, or `nil` when its generator raised |
| `:bits` | the bits of the shrunk case, as an array of 0 and 1 |
| `:choices` | the choice each draw of that case answered |
| `:minimal` | `false` when the cap stopped the shrinker, else `true` |
| `:original` | the value of the first failing case, before shrinking |
| `:error` | what the shrunk case raised, or `nil` when it returned false; the `:check-gave-up` error when `check:run` gave up |

```lisp
(def check ((import "std/check")))
(def r (check:run (check:int 0 1000) (fn [n] (< n 17))))
(assert (not (r :passed)))
(assert (= 17 (r :value)) "shrinking finds the smallest failure")
(assert (= [17] (r :choices)) "from one draw")
(assert (= [0 0 0 0 0 1 0 0 0 1] (r :bits)) "of 10 bits, for a bound of 1001")
(def ok (check:run (check:int 0 9) (fn [n] (< n 10)) :trials 5))
(assert (ok :passed))
```

A replay of the bits of 1000 shrinks to 17, unless the cap stops it. The
swap moves the 5 behind the zeros, because zeros first make simpler bits:

```lisp
(def check ((import "std/check")))
(def g (check:int 0 1000))
(def r (check:run g (fn [n] (< n 17)) :bits [1 1 1 1 1 0 1 0 0 0]))
(assert (= [1000 17 1 true] [(r :original) (r :value) (r :cases) (r :minimal)]))
(def again (check:run g (fn [n] (< n 17)) :bits (r :bits)))
(assert (= [17 17] [(again :original) (again :value)]) "a replay of :bits")
(def capped (check:run g (fn [n] (< n 17)) :bits [1 1 1 1 1 0 1 0 0 0] :shrinks 0))
(assert (= [1000 false] [(capped :value) (capped :minimal)]))
(def digits (check:array-of (check:int 0 9) 6 :min 3))
(def small? (fn [xs] (all? (fn [x] (< x 5)) xs)))
(def swapped (check:run digits small? :bits [0 0 0 1 0 1 0 0 0 0 0 0 0 0]))
(assert (= [[5 0 0] [0 0 5]] [(swapped :original) (swapped :value)]))
```

A generator that raises fails its case in the generator. Seed 1 first finds a
case that fails in the property, so its shrinking passes over 36:

```lisp
(def check ((import "std/check")))
(defn raises-at [bad]
  (fn [draw]
    (let [n (draw 1001)]
      (when (bad n) (error {:error :bad-draw :message (string n)}))
      n)))
(def r (check:run (raises-at (fn [n] (= n 36))) (fn [n] (< n 30)) :seed 1))
(assert (= [:property 30] [(r :failed-in) (r :value)]))
(def g (check:run (raises-at (fn [n] (>= n 500))) (fn [n] true) :seed 1))
(assert (= [:generator nil [500]] [(g :failed-in) (g :value) (g :choices)]))
(assert (= :bad-draw (get (g :error) :error)))
```

A run that gives up has `:passed` false and no counterexample. A filtered
generator shrinks to the simplest value its filter keeps:

```lisp
(def check ((import "std/check")))
(def evens (check:filter (check:int 0 1000) even?))
(assert (= 10 (get (check:run evens (fn [n] (< n 10))) :value)))
(def none (check:run (check:filter (check:int 0 9) (fn [_] false)) (fn [_] true)))
(assert (= [false 0 nil] [(none :passed) (none :cases) (none :value)]))
(assert (= :check-gave-up (get (none :error) :error)))
```

`(check:assert g prop ...)` takes the same arguments and returns `true` when
the property holds. When it fails, it raises `:failed-assertion`. The message
names the shrunk counterexample, or what its generator raised. It names the
seed, or says that the case came from replayed bits. The error's `:bits` holds
the bits of the shrunk case. When `check:run` gives up, `check:assert` raises
its `:check-gave-up` error.

```lisp
(def check ((import "std/check")))
(def [ok? err] (protect (check:assert (check:int 0 99) (fn [n] (< n 50)))))
(assert (not ok?))
(assert (= :failed-assertion (err :error)))
(assert (= [0 1 1 0 0 1 0] (err :bits)) "the seven bits of 50")
```
