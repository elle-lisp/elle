# ratchet

<!-- audited: 2026-10-04 -->

Measure a shape and print one reading line per subject, for the runner to
judge against the committed ledger.

[The design](../docs/ratchet.md) says why a bound never lives in a producer,
which build a row belongs to, and what the runner does with a line. This guide
is the producer's side. The instrument measures and prints, and judges
nothing, so nothing below asserts what a gauge read.

## One instrument per program

```lisp
(def r ((import "std/ratchet")))
(assert (fn? r:read) "the module answers an instrument")
(assert (= r:objects:axis :objects) "a gauge names the axis it reads")
(assert (= r:objects:unit "objects/op") "and the unit one point of a rate carries")
```

The instrument reads no ledger and needs neither the path it was started with
nor the build it runs on. A form in a worker thread, an isolated child and a
direct run all print the same lines.

## A reading is a number from anywhere

`read` takes a subject, an axis and a value, prints the reading, and answers
it. The unit defaults to the axis name, and `half` — the half-width of the
reading's interval — to 0.

```lisp
(def answer (r:read "answer" :count 42))
(assert (= answer:subject "answer") "the reading names its subject")
(assert (= answer:axis :count) "and its axis")
(assert (= answer:value 42) "and carries the value it was handed")
(assert (= answer:unit "count") "with the axis name for its unit")
(assert (= answer:half 0) "and an exact half-width")
(assert (nil? answer:verdict) "and no verdict: the runner judges")
```

Every reading prints one line, `measure` and a JSON object, and that line is
the rendering a human reads too. The ledger module reads it back:

```lisp
(def l ((import "std/ratchet/ledger")))
(def line (l:render-reading answer))
(assert (string/starts-with? line "measure {") "one line, opened by the word measure")
(def back (get (l:readings-in line) 0))
(assert (= back:subject "answer") "the line reads back to the reading")
(assert (= back:value 42) "value and all")
```

## A delta is a gauge's change over a window

`delta` runs `body` once uncounted, then `n` times inside the window, and
reports the gauge's change per run, one reading per gauge in `:on`.

```lisp
(def dropped (r:delta "dropped struct" (fn [] {:x 1}) :on [r:objects] :n 200))
(assert (= (length dropped) 1) "one reading per gauge")
(assert (= (get (get dropped 0) :subject) "dropped struct") "named for the shape")
(assert (= (get (get dropped 0) :unit) "objects/op") "in the gauge's unit")
```

The first reading on an axis drives that gauge's own live-growth shape first
and reports it as `objects gauge (live-growth)`, carrying `class` and
`floor`. The ledger holds it as a growth floor. A gauge that reads flat under
a shape that must grow is dead, and the runner voids every later reading on
that axis — so no producer writes a discriminator, and none can forget one.

## A rate is adaptive, with an interval

`rate` drives `(probe j)` in blocks and stops when the empirical-Bernstein
half-width on the per-op rate falls under `:epsilon`, or at `:max` blocks.
Its reading carries that half-width as `half`, and the judge compares the
whole interval to the bound.

```lisp
(def dropped-rate (r:rate "dropped struct rate" (fn [j] {:x j}) :block 50 :min 4 :max 10))
(assert (= (get (get dropped-rate 0) :axis) :objects) "on the default gauge")
(assert (number? (get (get dropped-rate 0) :half)) "with the half-width it reached")
```

`:on` names the gauges one drive is read on, `[r:objects]` by default. Two
gauges in one drive are two readings of the same operations, on two axes.
`:stable true` measures at two block sizes and voids a rate the block size
moves, which is a rate that is not a per-op rate at all.

## A drive is a run-block of the caller's own

`rate` runs `(probe j)` once per op. A shape whose ops are not a per-op thunk
hands `drive` a run-block, `(fn [b])`, that performs `b` ops: a tail recursion
that allocates once per call, a fiber drained after `b` yields, a loop that
has to run as a discarded statement. The reading is the same per-op rate, with
the same options. A run-block advances by intrinsics behind a guard, because a
variadic `+` inside the window is work the gauge would measure.

```lisp
(defn drop-n [b]
  (when (%not (%int? b)) (error :b))
  (def @i 0)
  (while (%lt i b)
    {:x i}
    (assign i (%add i 1))))
(def driven (r:drive "driven loop" drop-n :on [r:objects r:regions] :block 50 :min 4 :max 10))
(assert (= (length driven) 2) "one reading per gauge in :on")
(assert (= (get (get driven 1) :axis) :regions) "in the order given")
```

`stmt-run` wraps a thunk into the run-block that calls it `b` times as a
discarded statement, the while-loop shape a per-call strand needs to show; a
thunk's own return convention would reclaim the strand on its way out.

```lisp
(def stmt (r:drive "statement drop" (r:stmt-run (fn [] {:y 1})) :block 50 :min 4 :max 10))
(assert (= (get (get stmt 0) :subject) "statement drop") "stmt-run makes a run-block of a thunk")
```

## The judge

The judge lives in the ledger module, and the runner is its one caller.
`judge` is a pure function of a reading and a row. A pin is two-sided: a
reading past it the worse way is a `regression`, and one past it the better
way is `stale` until `elle-rig test --repin` moves the pin.

```lisp
(def pin {:kind :pin :bound 1.0 :better :lower :slack 0.0})
(assert (= (l:judge {:value 1.05 :half 0.1} pin) :ok) "the interval overlaps the pin")
(assert (= (l:judge {:value 1.3 :half 0.1} pin) :regression) "confidently above it")
(assert (= (l:judge {:value 0.0 :half 0.1} pin) :stale) "confidently below it: re-pin")
(assert (= (l:judge {:value 1.3 :half 0.5} pin) :ok) "a wide interval cannot fail a pin it straddles")
(assert (= (l:judge {:value 5 :half 0} nil) :unledgered) "no row")
(def floor {:kind :floor :bound 0.5 :better :lower :slack 0.0})
(assert (= (l:judge {:value 0.9 :half 0.2} floor) :ok) "a floor passes at or above it")
(assert (= (l:judge {:value 0.1 :half 0.2} floor) :regression) "and fails below it")
(def ceiling {:kind :ceiling :bound 8 :better :lower :slack 0.0})
(assert (= (l:judge {:value 9.0 :half 0.5} ceiling) :regression) "a ceiling fails above it")
(assert (= (l:judge {:value 8.0 :half 0.5} ceiling) :ok) "and never goes stale")
```

`:better :higher` swaps the sides of a pin, for a count that should climb.
`:slack` widens a pin on both sides for a subject the machine makes noisy; it
is not the estimator's uncertainty, which every rate already carries.

## Exports

| Export | What it does |
|--------|--------------|
| `objects`, `regions`, `bytes`, `pages`, `ids` | the arena gauges, each with its axis, unit, live-growth shape and floor |
| `(gauge axis unit read &named disc floor epsilon)` | a gauge of the caller's own; `disc` is its live-growth probe, or nil for none |
| `(read subject axis value &named unit half)` | one reading, printed and answered |
| `(delta subject body &named on n)` | a gauge's change per run of `body` over `n` runs, on each gauge in `on` |
| `(rate subject probe &named on epsilon block min max stable)` | the adaptive per-op rate of `(probe j)`, on each gauge in `on` |
| `(drive subject run-block &named on epsilon block min max stable)` | the same rate over `(run-block b)`, which performs `b` ops of the caller's own shape |
| `(stmt-run thunk)` | the run-block that calls `thunk` `b` times as a discarded statement |
| `(ratio subject measured control &named rounds)` | the best of `rounds` alternating timings of `measured` over the best of `control`, on the `:time` axis |
| `readings` | every reading this instrument printed, in order |
