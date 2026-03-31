# LIR — Low-level IR

LIR is an SSA-form intermediate representation with virtual registers,
basic blocks, and explicit control flow.

## Key types

- **`LirFunction`** — a function body: entry label, blocks, metadata
  (arity, locals, captures, cell/lbox masks)
- **`BasicBlock`** — a sequence of `LirInstr` followed by a
  `Terminator`
- **`Reg`** — virtual register (SSA — each assigned exactly once)
- **`Label`** — block label for control flow
- **`LirInstr`** — individual operations (load const, add, call, etc.)
- **`Terminator`** — block-ending instruction (return, jump,
  branch, tail call)
- **`LirConst`** — compile-time constants (int, float, string,
  keyword, nil, true, false)

## From HIR to LIR

The lowerer (`src/lir/lower/`) transforms HIR trees into LIR:

1. **Flatten** — nested expressions → linear instruction sequences
2. **Register allocation** — each intermediate value gets a virtual
   register
3. **Block construction** — control flow (if, loops, match) creates
   basic blocks connected by terminators
4. **Escape analysis** — determines which scopes can use region-based
   allocation (RegionEnter/RegionExit)

## Yield metadata

LIR collects yield-site and call-site information during emission.
The JIT uses this for yield-through-call support — knowing which
calls might yield so it can emit proper save/restore sequences.

## Files

```text
src/lir/types.rs          LirFunction, BasicBlock, Reg, etc.
src/lir/display.rs        Debug printing of LIR
src/lir/lower/            Lowering passes
```

---

## See also

- [impl/hir.md](hir.md) — HIR analysis before lowering
- [impl/bytecode.md](bytecode.md) — bytecode emitted from LIR
- [impl/jit.md](jit.md) — JIT translates LIR directly
