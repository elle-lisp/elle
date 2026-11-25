# The callees of the region IR

<!-- audited: 2026-11-24 -->

How a call of the region IR names its callee, and the facts a native callee's signature gives at the call.

[The facts](facts.md) gives the rest of each step's facts, and
[call](call.md) reads `call` and `arg` beside the callee's signature.

## Callees

A call names its callee by what wrote the callee's register:

| The register's writer | The callee |
|-----------------------|------------|
| a ValueConst of a native | the native's name in `compile/primitives` |
| a ValueConst of a closure | `[:constant k]`, the closure constant `k` of the unit, numbered in order of first sight and compared with `identical?` |
| a MakeClosure of the closure id `c` | `[:closure c]` |
| a LoadSelf in the function whose closure id is `c` | `[:closure c]` |
| anything else | the register |

The closure constants are numbered across the unit, so two functions that call
one constant name it alike.

## A call

A call gives `call(d, f, p)`, `f` its callee, and `arg(y, i, p)` for each
argument `y` at position `i`. A native callee also gives the facts of
[its signature](signature.md), its `:spec`, read at the call. A resumed
parameter gives `resume(y, p)`, a crossing parameter `cross(y, p)`, and a
payload result `payload(d, y, p)`. A fresh result gives `alloc(d, p)`, and a
fresh immutable result also gives `immutable(d)` and `operand(y, d, p)` for
each argument at a returned parameter. A parameter stored into a place gives
`store(y, c, p)`, `c` the argument `j` for `[:param j]` or `d` for `:result`.
Every other mode and kind is the callee's signature, which [call](call.md)
reads.
