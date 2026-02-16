# lir

Low-level Intermediate Representation. SSA form with virtual registers
and basic blocks. Architecture-independent but close to target.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `BindingId` references to concrete slot indices
- Emit cell operations for mutable captures
- Produce bytecode via `Emitter`

Does NOT:
- Resolve bindings (that's HIR)
- Execute code (that's VM)
- Perform optimization (future work)

## Interface

| Type | Purpose |
|------|---------|
| `LirFunction` | Compilation unit: blocks, constants, metadata |
| `BasicBlock` | Instructions + terminator |
| `LirInstr` | Individual operation |
| `Terminator` | How block exits: `Return`, `Jump`, `Branch` |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `Lowerer` | HIR → LIR |
| `Emitter` | LIR → Bytecode |

## Data flow

```
HIR + bindings
    │
    ▼
Lowerer
    ├─► allocate slots for bindings
    ├─► emit MakeCell for captured locals
    ├─► lower control flow to jumps
    └─► emit LoadCapture/StoreCapture for upvalues
    │
    ▼
LirFunction (basic blocks)
    │
    ▼
Emitter
    ├─► simulate stack for register→stack translation
    ├─► patch jump offsets
    └─► emit Instruction bytes
    │
    ▼
Bytecode
```

## Dependents

- `pipeline.rs` - uses `Lowerer` and `Emitter`
- `vm/` - executes the emitted bytecode

## Invariants

1. **Each register assigned exactly once.** SSA form. If you see a register
   used before definition, lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, or
   `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with
   "unknown binding," the HIR→LIR mapping is incomplete.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside lambdas,
   captures and parameters are upvalues; they use LoadCapture, not LoadLocal.

5. **`cell_params_mask` is set for mutable parameters.** Bit i set means
   parameter i needs cell wrapping at call time.

## Key instructions

| Instruction | Stack effect | Notes |
|-------------|--------------|-------|
| `LoadLocal` | → value | Load from stack slot |
| `StoreLocal` | value → value | Store to slot, keep on stack |
| `LoadCapture` | → value | From closure env, auto-unwraps LocalCell |
| `LoadCaptureRaw` | → cell | From closure env, preserves cell (for forwarding) |
| `StoreCapture` | value → | Into closure env, handles cells |
| `MakeCell` | value → cell | Wrap in LocalCell |
| `MakeClosure` | caps... → closure | Pops N captures, creates closure |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 20 | Re-exports |
| `types.rs` | 270 | `LirFunction`, `LirInstr`, `Reg`, `Label`, etc. |
| `lower.rs` | 1400+ | `Lowerer`, HIR→LIR translation |
| `emit.rs` | 800 | `Emitter`, LIR→Bytecode with stack simulation |
