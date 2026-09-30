# ratchet

<!-- audited: 2026-09-30 -->

Measure a shape, print one reading line per subject, and judge each reading
against the committed ledger.

`elle test` records and re-pins the same lines.

[The design](../docs/ratchet.md) says why a bound never lives in a producer
and what `elle test` adds. This guide is the producer's side, and it is a
producer: every reading below is judged against
[its own ledger](../tests/ledger/guide.lisp).

## One instrument per program

```lisp
(def r ((import "std/ratchet")))
(assert (fn? r:read) "the module answers an instrument")
(assert (= r:objects:axis :objects) "a gauge names the axis it reads")
(assert (= r:objects:unit "objects/op") "and the unit one point of a rate carries")
```

The instrument finds its producer in the path the program was started with,
and its ledger under `tests/ledger` in the tree the binary was built in.
`ELLE_LEDGER` names another directory, which is how a test hands a producer a
ledger of its own. A program started with no path, such as a form inside a
worker thread, has no producer: it prints every reading without a verdict and
leaves the judging to the runner.

## A reading is a number from anywhere

`read` takes a subject, an axis and a value. The unit defaults to the axis
name, and `half` — the half-width of the reading's interval — to 0.

```lisp
(def answer (r:read "answer" :count 42))
(assert (= answer:verdict :ok) "42 is what the ledger pins")
(assert (= answer:bound 42) "the row it met")
```

Every reading prints one line, `measure` and a JSON object, and that line is
the rendering a human reads too:

```text
measure {"subject":"answer","axis":"count","value":42,"half":0,"unit":"count","bound":42,"kind":"pin","verdict":"ok"}
```

## A delta is a gauge's change over a window

`delta` runs `body` once uncounted, then `n` times inside the window, and
reports the gauge's change per run. A struct built and dropped leaves nothing,
so the object count reads 0.

```lisp
(def dropped (r:delta "dropped struct" (fn [] {:x 1}) :on [r:objects] :n 200))
(assert (= (get (get dropped 0) :verdict) :ok) "a dropped struct costs 0 objects/op")
```

The first reading on an axis drives that gauge's own live-growth shape first
and reports it as `objects gauge (live-growth)`. The ledger holds it as a
growth floor. A gauge that reads flat under a shape that must grow is dead,
and every later reading on that axis is `void` — so no producer writes a
discriminator, and none can forget one.

## A rate is adaptive, with an interval

`rate` drives `(probe j)` in blocks and stops when the empirical-Bernstein
half-width on the per-op rate falls under `:epsilon`, or at `:max` blocks.
Its reading carries that half-width as `half`, and the judge compares the
whole interval to the bound.

```lisp
(def dropped-rate (r:rate "dropped struct rate" (fn [j] {:x j}) :block 50 :min 4 :max 10))
(assert (= (get (get dropped-rate 0) :verdict) :ok) "0 objects/op, at 0 ±0")
```

`:on` names the gauges one drive is read on, `[r:objects]` by default. Two
gauges in one drive are two readings of the same operations, on two axes.
`:stable true` measures at two block sizes and voids a rate the block size
moves, which is a rate that is not a per-op rate at all.

## The judge

`judge` is a pure function of a reading and a row, and it is the one every
verdict comes from, here and in the runner. A pin is two-sided: a reading past
it the worse way is a `regression`, and one past it the better way is `stale`
until `elle test --repin` moves the pin.

```lisp
(def pin {:kind :pin :bound 1.0 :better :lower :slack 0.0})
(assert (= (r:judge {:value 1.05 :half 0.1} pin) :ok) "the interval overlaps the pin")
(assert (= (r:judge {:value 1.3 :half 0.1} pin) :regression) "confidently above it")
(assert (= (r:judge {:value 0.0 :half 0.1} pin) :stale) "confidently below it: re-pin")
(assert (= (r:judge {:value 1.3 :half 0.5} pin) :ok) "a wide interval cannot fail a pin it straddles")
(assert (= (r:judge {:value 5 :half 0} nil) :unledgered) "no row")
(def floor {:kind :floor :bound 0.5 :better :lower :slack 0.0})
(assert (= (r:judge {:value 0.9 :half 0.2} floor) :ok) "a floor passes at or above it")
(assert (= (r:judge {:value 0.1 :half 0.2} floor) :regression) "and fails below it")
(def ceiling {:kind :ceiling :bound 8 :better :lower :slack 0.0})
(assert (= (r:judge {:value 9.0 :half 0.5} ceiling) :regression) "a ceiling fails above it")
(assert (= (r:judge {:value 8.0 :half 0.5} ceiling) :ok) "and never goes stale")
```

`:better :higher` swaps the sides of a pin, for a count that should climb.
`:slack` widens a pin on both sides for a subject the machine makes noisy; it
is not the estimator's uncertainty, which every rate already carries.

## The report

`report` fails once, naming every reading that is not `ok` and every row of
this producer that got no reading. A producer ends with it.

```lisp
(r:report)
```

## Exports

| Export | What it does |
|--------|--------------|
| `objects`, `regions`, `bytes`, `pages`, `ids` | the arena gauges, each with its axis, unit, live-growth shape and floor |
| `(gauge axis unit read &named disc floor epsilon)` | a gauge of the caller's own; `disc` is its live-growth probe, or nil for none |
| `(read subject axis value &named unit half)` | one reading, judged and printed |
| `(delta subject body &named on n)` | a gauge's change per run of `body` over `n` runs, on each gauge in `on` |
| `(rate subject probe &named on epsilon block min max stable)` | the adaptive per-op rate of `(probe j)`, on each gauge in `on` |
| `(ratio subject measured control &named rounds)` | the best of `rounds` alternating timings of `measured` over the best of `control`, on the `:time` axis |
| `(judge reading row)` | the verdict of one reading against one row, or `:unledgered` for no row |
| `(judge-all readings rows)` | every reading in order, with a failed growth floor voiding its axis |
| `(readings-in text)` | the readings printed in captured output, one per `measure` line |
| `(report)` | fail once with every reading that is not `ok` and every row left unread |
