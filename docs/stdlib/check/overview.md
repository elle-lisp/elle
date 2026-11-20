# check

<!-- audited: 2026-11-20 -->

The submodules behind [check.lisp](../../../lib/check.lisp): bits, sources, the walk, generators, a case, the shrinker, and the runner.

[check.md](../check.md) holds what a caller of the module sees. This file
holds how the module divides.

| File | Purpose |
|------|---------|
| [bits.lisp](../../../lib/check/bits.lisp) | A sequence of bits: the width of a bound, writing and reading a choice, splicing, and which of two sequences is simpler |
| [errors.lisp](../../../lib/check/errors.lisp) | The errors the module raises, each from one constructor |
| [source.lisp](../../../lib/check/source.lisp) | The recording source behind `draw`, the record it keeps, and the replay of one bit sequence |
| [random.lisp](../../../lib/check/random.lisp) | The SplitMix64 stream, and the random sources it feeds, which decide on edges |
| [walk.lisp](../../../lib/check/walk.lisp) | The depth-first walk over every choice sequence up to a size, as the next source after each case |
| [gen.lisp](../../../lib/check/gen.lisp) | The generators, each a function of `draw`, `assume`, and the parameter that collects the call logs of a case's generated functions |
| [case.lisp](../../../lib/check/case.lisp) | A case: the value a generator makes from a source, whether the property passes, fails or skips on it, and the calls of its functions |
| [shrink.lisp](../../../lib/check/shrink.lisp) | The shrinker: its steps, each a function of the case it holds and one draw, and the sweeps that run them |
| [run.lisp](../../../lib/check/run.lisp) | The options, the search over phases of sources, and `run`, `assert`, `enumerate` and `sample` above it |

A source decides each draw one of two ways. It reads the draw's choice from
bits, or a decider names the choice, and the source records that choice's
bits at the draw's width. A random source's decider names an edge from stream
bits it does not record, so a replay of the recorded bits needs no edges.

The record keeps one array per field, with one entry per draw: the bound, the
choice, where the draw's bits start, where its accepted bits start, and where
they end. One more array holds, for the draw that chose the length of a
repeated draw, the `[start end]` of each element's bits. The walk reads the
record to find the next sequence, and the shrinker reads it to find what to
delete and what to lower.

A search runs phases in turn. A phase answers the source of its next case from
the case before, and may bound how many cases pass and how many skip. The
enumeration, the random cases and a replay are three phases, and `run`,
`enumerate` and `sample` each search one or two of them.

A submodule takes the submodules it needs as arguments, and
[check.lisp](../../../lib/check.lisp) makes one of each. Two loads of one file
build two values. A random source and a replay from two loads of
[source.lisp](../../../lib/check/source.lisp) hand a generator two compiled
`draw` functions, and its call site then runs three times slower on the JIT.
One instance of [gen.lisp](../../../lib/check/gen.lisp) also gives the
generators and a case one parameter to collect call logs in:

```lisp
(def bits ((import "std/check/bits")))
(def errors ((import "std/check/errors")))
(def source ((import "std/check/source") :bits bits :errors errors))
(def random ((import "std/check/random") :bits bits :source source))
(def walk ((import "std/check/walk") :bits bits :source source))
(def gen ((import "std/check/gen") :errors errors))
(def case ((import "std/check/case") :errors errors :gen gen))
(def shrink ((import "std/check/shrink") :bits bits :source source :case case))
(def run
  ((import "std/check/run") :bits bits :errors errors :source source
                            :random random :walk walk :case case
                            :shrink shrink))
(assert (= [0 1 2] (run:enumerate (gen:int 0 9) 2)))
```
