# LIR — Low-level IR

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
   (see `docs/regions.md`); the lowerer emits `DecrefRegion` at each
   region's `free_at` HirId and `IncrefRegion` at cross-region edges

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

A template carries symbols **by name**, not by interned id: ids are
per-symbol-table (per-instance), so a raw id would name a different symbol after
the template crosses a `sys/spawn` boundary. `materialize` re-interns the name
into the executing instance's table (the explicitly-threaded `SymbolTable`),
exactly as a sent symbol `Value` re-interns. This makes the template
self-contained and portable.

See [region-model.md](region-model.md) — *Constants lower as ordinary
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
