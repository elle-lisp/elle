# Low-level Intermediate Representation (LIR)

<!-- audited: 2026-09-23 -->

LIR sits between HIR and bytecode: virtual registers and basic blocks that make control flow explicit.

## Two phases

**Lowering** (HIR → LIR): the `Lowerer` walks HIR and produces LIR. It gives
each binding a slot, puts a captured mutable binding in a capture cell, turns
`if`, `while`, `match` and the rest into jumps between blocks, and emits the
region instructions the region solver placed.
[lower/AGENTS.md](lower/AGENTS.md) describes it.

**Emission** (LIR → bytecode): the `Emitter` turns register-based LIR into
stack-based bytecode. It simulates the operand stack to find where each
register sits, copies a value to the top with `DupN` when it is not already
there, and patches jump offsets once every block is placed.
[AGENTS.md](AGENTS.md) describes it.

## Registers and the stack

Each virtual register (`r0`, `r1`, ...) is assigned exactly once. The emitter
places registers on the operand stack, so an addition of two constants becomes
two loads and one `Add`. Run `elle --dump=lir FILE` to see the LIR of a
program:

```sh
elle --dump=lir script.lisp
```

## Capture cells

A binding that a closure captures and that `assign` also writes lives in a
capture cell, so that each side sees the other's writes. Only an `@`-prefixed
binding can be written, so an ordinary captured binding is copied into the
closure by value. An immutable binding whose initializer is a literal goes
further: the lowerer loads its value as a constant and reads no slot at all.

```lisp
(let [@counter 0]
  (def inc (fn () (assign counter (+ counter 1))))
  (inc)
  (assert (= counter 1) "the closure's write reaches the outer binding"))
```

## Lambdas

Each lambda lowers to its own `LirFunction`, held in the module's closure list.
`MakeClosure` names it by `ClosureId` and takes the captured values from
registers.

## See also

- [AGENTS.md](AGENTS.md) — the LIR types, the emitter and its invariants
- [docs/impl/lir.md](../../docs/impl/lir.md) — the design of LIR
- [src/hir/](../hir/) — the input to lowering
- [src/compiler/bytecode.rs](../compiler/bytecode.rs) — the bytecode instructions
- [src/vm/](../vm/) — executes the bytecode
