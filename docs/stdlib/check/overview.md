# check

<!-- audited: 2026-11-18 -->

The submodules behind [check.lisp](../../../lib/check.lisp): the sources a generator draws from, the generators, and the runner.

[check.md](../check.md) holds what a caller of the module sees. This file
holds how the module divides.

| File | Purpose |
|------|---------|
| [source.lisp](../../../lib/check/source.lisp) | The three sources `draw` reads: a seeded SplitMix64 stream, a depth-first walk over every choice sequence up to a size, and a replay of one sequence |
| [gen.lisp](../../../lib/check/gen.lisp) | The generators, each a function of `draw` |
| [run.lisp](../../../lib/check/run.lisp) | `run`, `assert`, `enumerate`, `sample`, and the shrinker |

Each source records the choices it answered and the bound of each draw. The
enumeration reads that record to find the next sequence, and the shrinker
reads it to find what to delete and what to lower.

A submodule takes the submodules it needs as arguments:

```lisp
(def source ((import "std/check/source")))
(def gen ((import "std/check/gen")))
(def run ((import "std/check/run") :source source))
(assert (= [0 1 2] (run:enumerate (gen:int 0 9) 2)))
```
