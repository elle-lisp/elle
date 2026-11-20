# check

<!-- audited: 2026-11-20 -->

The submodules behind [check.lisp](../../../lib/check.lisp): the sources a generator draws from, the generators, the shrinker, and the runner.

[check.md](../check.md) holds what a caller of the module sees. This file
holds how the module divides.

| File | Purpose |
|------|---------|
| [source.lisp](../../../lib/check/source.lisp) | The three sources `draw` reads: random bits from a seeded SplitMix64 stream, a depth-first walk over every choice sequence up to a size, and a replay of one bit sequence |
| [gen.lisp](../../../lib/check/gen.lisp) | The generators, each a function of `draw`, and `assume` |
| [shrink.lisp](../../../lib/check/shrink.lisp) | A case: the value a generator makes from a source, and whether the property passes, fails or skips on it. The shrinker |
| [run.lisp](../../../lib/check/run.lisp) | `run`, `assert`, `enumerate` and `sample` |

Each source records the bits it read, and for each draw its bound, its
choice, and where its rejected bits and its accepted bits begin. The draw
that chose the length of a repeated draw also records where each element's
bits begin and end. The enumeration reads that record to find the next
sequence, and the shrinker reads it to find what to delete and what to lower.

A submodule takes the submodules it needs as arguments:

```lisp
(def source ((import "std/check/source")))
(def gen ((import "std/check/gen")))
(def shrink ((import "std/check/shrink") :source source))
(def run ((import "std/check/run") :source source :shrink shrink))
(assert (= [0 1 2] (run:enumerate (gen:int 0 9) 2)))
```
