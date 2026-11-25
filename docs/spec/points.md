# The points of the region IR

<!-- audited: 2026-11-24 -->

Where each step of a function of the region IR lies, which step follows it, and what a jump hands to a block's parameters.

[The facts](facts.md) says what each step gives besides, and
[datalog](datalog.md) defines `next`, `pass` and `alloc`.

## Points

A point is an instruction or a terminator, numbered in view order: the blocks
in the order of `:blocks`, and each block's instructions before its
terminator. `next` follows instruction order and each terminator's targets.
Two kinds of point have no node of their own:

| Point | What it is |
|-------|------------|
| `[:exit p]` | the exit of the Return or the TailCall at `p` |
| `[:edge p l]` | the edge from the Branch at `p` to the block `l`, where `l` takes parameters |

Each Return has an exit of its own, because a shared exit would keep a value
returned on one path live behind every other return. A TailCall returns at its
own point, because a closure callee replaces the frame there. Its fall-through
runs only for a native callee.

## Terminators and block parameters

A block parameter is a register its block defines. A jump hands it a value,
and so the step that hands it is a `pass`, as the Recur rule of
[the ownership rules](ownership.md) reads one.

| Kind | Facts |
|------|-------|
| TailCall | `next(p, [:exit p])`, beside the `next` to the instruction after it |
| Return | `next(p, [:exit p])` |
| Jump | `next` to its target, and `pass(y, x, p)` for each argument `y` and the target's parameter `x` at its position |
| Branch | `next` to the target that takes no parameter; `next(p, [:edge p l])` to a target `l` that takes some |
| an edge point `[:edge p l]` | `next` to the first point of `l`, and `pass(y, x, [:edge p l])` for each argument `y` of the Branch to `l` |
| Emit | `next` to its resume block, and `alloc(x, p)` for that block's one parameter `x` |
| Unreachable | no `next` |

A Branch hands its arguments on the edge, because only one edge runs. A pass
at the Branch's own point would move each argument on both edges, and release
the other target's parameter as unused. An Emit's resume value comes from the
fiber that resumes this one, and a resume moves its value, so this fiber owns
it from the Emit on.
