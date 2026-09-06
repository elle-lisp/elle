# LIR — Low-level IR

<!-- audited: 2026-09-06 -->

LIR is an SSA-form intermediate representation with virtual registers,
basic blocks, and explicit control flow.

## Key types

- **`LirFunction`** — a function body: entry label, blocks, metadata
  (arity, locals, captures, capture-cell masks, signal, region table)
- **`BasicBlock`** — a sequence of `LirInstr` followed by a
  `Terminator`
- **`Reg`** — virtual register (SSA — each assigned exactly once)
- **`Label`** — block label for control flow
- **`LirInstr`** — individual operations (load const, add, call, etc.)
- **`Terminator`** — block-ending instruction (return, jump,
  branch, emit, unreachable). Note: tail calls are `LirInstr`
  variants (`TailCall`/`TailCallArrayMut`), not terminators.
- **`LirConst`** — compile-time **immediate** constants (int, float,
  nil, true, false; plus interned keyword/symbol — all tag+payload, no
  heap). `Const`/`ValueConst` are pure pool loads with no `region` field.
- **`MaterializeConst`** — the allocation that builds a *heap* literal
  (a string, or quoted compound data: list / array / nested structure) from
  a recursive immutable `ConstTemplate` (`src/value/template.rs`) into **its
  own** solver-assigned region. It carries a mandatory `region: StaticRegion`
  and is an ordinary allocation site (see *Heap literals are allocations*
  below). The whole aggregate shares the one region (built bottom-up, so every
  internal reference is a self-edge taking no cross-region RC).

## From HIR to LIR

The lowerer (`src/lir/lower/`) transforms HIR trees into LIR:

1. **Flatten** — nested expressions → linear instruction sequences
2. **Register allocation** — each intermediate value gets a virtual
   register
3. **Block construction** — control flow (if, loops, match) creates
   basic blocks connected by terminators
4. **Region assignment** — every allocation is routed to a region
   (see [regions](../regions.md)); the lowerer emits `DecrefRegion` at each
   region's `free_at` HirId and `IncrefRegion` at cross-region edges

## The operand proof

A `%`-intrinsic in call position compiles only when the front end discharges its
operand contract ([intrinsics.md](../intrinsics.md)). That proof used to stop at
this boundary, and every backend then re-derived at run time what the compiler
had already decided.

`BinOp`, `Compare` and `UnaryOp` carry it across instead. `OperandProof::Int`
says every operand of that instruction is an integer on every path reaching it.
`OperandProof::Unproven` claims nothing. The lowerer reads each operand node's
inferred type out of `TypeInfo` — the same map the contract check discharged
against — and marks the instruction `Int` when every one of them is exactly
`int`. Nothing downstream may set the proof: a backend that guessed would be
asserting what only the front end can know.

Build an instruction with `LirInstr::binop`, `compare` or `unary` to claim
nothing, and with `int_binop`, `int_compare` or `int_unary` to carry the proof.

What each backend spends it on:

| Backend | Unproven | Int |
|---------|----------|-----|
| bytecode | `Add` `Sub` `Mul` `Div` | `AddInt` `SubInt` `MulInt` `DivInt` |
| JIT | tag check, then the integer path or a helper call | the integer instruction alone |
| WASM | tag check, then the `i64` path or the `f64` path | the `i64` instruction alone |
| MLIR, SPIR-V | operand types from local inference | the integer operation |

The bytecode set specializes four operations, so a proven `%rem`, `%bit-and` or
comparison still emits the polymorphic opcode. Those handlers already read their
operands as integers and do no better with the proof.

A division keeps its zero test on every tier. The contract does prove the divisor
nonzero wherever both operands are proven integers, but `OperandProof` names the
operand type and says nothing about a value, and a trapping `sdiv` is the wrong
place to spend a reading it does not carry.

An unproven instruction is correct everywhere and only slower, so a lowering that
cannot decide says `Unproven` and each backend does what it did before.

## Heap literals are allocations

A heap literal is an ordinary allocation, **not** a pre-baked `Value`. The
constant pool stores only the literal's immutable *template* — a
recursive `ConstTemplate` (string bytes; or a quoted list/array/nested structure;
plus the closure template) as compile-time data, encoded inline in the
(reclaimable) bytecode and held as a `Box<ConstTemplate>` by `JitCode`.
`MaterializeConst` reads that template and allocates a fresh heap value every time
it runs into **its own** region — the solver gives each literal its own
`region: StaticRegion` (`alloc_here`) and `decref_point` exactly like `List`/
`MakeArrayMut`/`MakeClosure`, resolved per activation to a fresh physical region
and allocated into that explicit region (`alloc_in_region`). A quoted aggregate's whole
structure shares that one region. Normal escape RC keeps any escaped copy alive
past its `decref_point`. Only immediates (numbers, bools, nil, interned
keyword/symbol) remain as plain pool constants loaded by `Const`/`ValueConst`.

A template carries symbols **by name**, not by interned id. The id would in fact
survive a `sys/spawn` boundary now that it is the name's hash
([symbol.md](symbol.md)), but the name is what makes the template readable and
self-describing; `materialize` interns it, which records the spelling for
display and returns that same id.

See [region/model.md](region/model.md) — *Constants lower as ordinary
allocations* — for why a code-object-lifetime "constant-pool region" is forbidden.

## Self-reference: `LoadSelf`

A closure that references itself in **value** position — passed to a
higher-order call, returned, or stored, then invoked later — lowers that
reference to `LoadSelf { dst }`. The op takes no operand and pushes the
**currently-executing closure**: the runtime holds the executing closure in a
per-activation register (`current_closure`, `src/value/fiber.rs`), and the JIT
receives that same closure value as a compiled-body parameter, so `LoadSelf`
reads it directly rather than naming a capture slot. The value it yields is the
closure itself, so an invocation of that value recurses correctly
(`src/runtime/tests/selfrec.rs`, `tests/elle/recur-{as-value,after-tail-call}.lisp`).

A self-reference in **call** position (`(loop args)`) is not this op: the call
lowers through the ordinary callee path. `LoadSelf` is the value path only.

`LirFunction` collects yield-point (`yield_points`) and call-site
(`call_sites`) information during bytecode emission. The JIT uses this
for yield-through-call support — knowing which calls might suspend so
it can generate proper save/restore sequences.

## Files

```text
src/lir/types/            LirFunction (func.rs), LirInstr (instr.rs),
                          BasicBlock, Reg, Terminator, LirConst (mod.rs)
src/lir/display.rs        Debug printing of LIR
src/lir/lower/            Lowering passes
src/lir/emit/             Bytecode emission from LIR
```

---

## See also

- [impl/hir.md](hir.md) — HIR analysis before lowering
- [impl/bytecode.md](bytecode.md) — bytecode emitted from LIR
- [impl/jit.md](jit.md) — JIT translates LIR directly
