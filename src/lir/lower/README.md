# LIR Lowering

The lowering phase transforms HIR into LIR by allocating stack slots, determining lbox boxing requirements, and translating control flow into explicit jumps and labels.

## Two-Phase Lowering

**Phase 1: Slot Allocation**
- Allocate stack slots for local variables
- Build `binding_to_slot` map for variable access
- Determine which bindings need lbox boxing

**Phase 2: Code Generation**
- Translate HIR expressions to LIR instructions
- Convert control flow (`if`, `while`, etc.) to jumps and labels
- Handle closure creation and capture loading
- Emit lbox operations for mutable captures

## LBox Boxing

When a variable is both captured by a closure AND mutated, it needs lbox boxing:

```janet
(let [counter 0]
  (def inc (fn () (set counter (+ counter 1))))
  (inc)
  counter)  ; Should be 1, not 0
```

The lowerer:
1. Detects that `counter` is captured and mutated
2. Emits `MakeCaptureCell` to wrap the initial value
3. Emits `LoadCaptureCell`/`StoreCaptureCell` for access in the outer scope
4. Emits `LoadCapture`/`StoreCapture` for access in the closure

## Key Files

| File | Purpose |
|--------|---------|
| [`mod.rs`](mod.rs) | `Lowerer` struct, context, entry point |
| [`expr.rs`](expr.rs) | Expression lowering: literals, operators, calls |
| [`binding.rs`](binding.rs) | Binding forms: `let`, `def`, `var`, `fn` |
| [`lambda.rs`](lambda.rs) | fn lowering, closure capture, lbox wrapping |
| [`control.rs`](control.rs) | Control flow: `if`, `begin`, `match` |
| [`pattern.rs`](pattern.rs) | Pattern matching lowering |
| [`escape.rs`](escape.rs) | Escape analysis for scope allocation |

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/lir/`](../) - LIR types and overview
- [`src/lir/emit.rs`](../emit.rs) - LIR to bytecode emission
