# Low-level Intermediate Representation (LIR)

LIR sits between HIR and bytecode. It uses virtual registers and basic blocks
to represent explicit control flow, making it straightforward to emit the
final stack-based bytecode.

## Two-Phase Translation

**Lowering** (HIR → LIR): The `Lowerer` walks HIR and produces LIR instructions.
This phase:
- Allocates stack slots for local variables
- Determines which bindings need cell boxing
- Translates control flow (if, while, etc.) into jumps and labels
- Handles closure creation and capture loading

**Emission** (LIR → Bytecode): The `Emitter` converts register-based LIR to
stack-based bytecode. It:
- Simulates a stack to track register positions
- Emits `DupN` when values aren't in expected positions
- Patches jump offsets after all instructions are emitted

## Register vs Stack

LIR uses virtual registers (`Reg(0)`, `Reg(1)`, ...) for clarity. Each register
is assigned exactly once (SSA form). The emitter translates these to stack
operations:

```
LIR:                          Bytecode:
  Const { dst: Reg(0), 42 }     LoadConst 42    ; push 42
  Const { dst: Reg(1), 10 }     LoadConst 10    ; push 10
  BinOp { Add, Reg(0), Reg(1) } Add             ; pop 10, pop 42, push 52
```

## Cell Boxing

When a variable is both captured by a closure AND mutated, it needs cell
boxing so mutations are visible across closure boundaries:

```lisp
(let ((counter 0))
  (def inc (fn () (set! counter (+ counter 1))))
  (inc)
  counter)  ; Should be 1, not 0
```

The lowerer:
1. Detects that `counter` is captured and mutated
2. Emits `MakeCell` to wrap the initial value
3. Emits `LoadCell`/`StoreCell` for access in the outer scope
4. Emits `LoadCapture`/`StoreCapture` for access in the closure

## Lambda Lowering

Lambdas are lowered recursively into separate `LirFunction`s:

1. Save current lowerer state
2. Create new function with parameters as upvalues
3. Lower body
4. Restore state
5. Emit `MakeClosure` with captured values

The closure template goes into constants; captures are pushed on the stack
and popped by `MakeClosure`.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/hir/` - input to LIR lowering
- `src/compiler/bytecode.rs` - instruction definitions
- `src/vm/` - executes the bytecode
