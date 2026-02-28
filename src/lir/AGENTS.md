# lir

Low-level Intermediate Representation. SSA form with virtual registers
and basic blocks. Architecture-independent but close to target.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `Binding` references to concrete slot indices
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
| `SpannedInstr` | `LirInstr` + `Span` for source tracking |
| `SpannedTerminator` | `Terminator` + `Span` for source tracking |
| `Terminator` | How block exits: `Return`, `Jump`, `Branch`, `Yield` |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `Lowerer` | HIR → LIR |
| `Emitter` | LIR → Bytecode + LocationMap |

## Data flow

```
HIR + spans
    │
    ▼
Lowerer
    ├─► allocate slots for bindings (HashMap<Binding, u16>)
    ├─► emit MakeCell for captured locals (binding.needs_cell())
    ├─► lower control flow to jumps
    ├─► emit LoadCapture/StoreCapture for upvalues
    └─► propagate HIR spans to SpannedInstr
    │
    ▼
LirFunction (basic blocks with SpannedInstr)
    │
    ▼
Emitter
    ├─► simulate stack for register→stack translation
    ├─► patch jump offsets
    ├─► emit Instruction bytes
    └─► build LocationMap from SpannedInstr spans
    │
    ▼
Bytecode + LocationMap
```

The lowerer reads binding metadata directly from `Binding` objects (via
`binding.needs_cell()`, `binding.is_global()`, `binding.name()`, etc.)
rather than looking up a separate bindings HashMap.

## Source location tracking

`SpannedInstr` wraps `LirInstr` with a `Span` for source location tracking:

```rust
pub struct SpannedInstr {
    pub instr: LirInstr,
    pub span: Span,
}
```

The lowerer propagates HIR spans to LIR instructions. The emitter builds a
`LocationMap` that maps bytecode offsets to source locations. This map is
stored in `Closure.location_map` and used by the VM for error reporting.

## Dependents

- `pipeline.rs` - uses `Lowerer` and `Emitter`
- `vm/` - executes the emitted bytecode

## Invariants

1. **Each register assigned exactly once.** SSA form. If you see a register
   used before definition, lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Yield`,
   or `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with
   "unknown binding," the HIR→LIR mapping is incomplete. The key is `Binding`
   (hashed by `Value::to_bits()`), the value is `u16` slot index.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside fn bodies,
   captures and parameters are upvalues; they use LoadCapture, not LoadLocal.

5. **`cell_params_mask` is set for mutable parameters.** Bit i set means
   parameter i needs cell wrapping at call time.

6. **`cell_locals_mask` is set for locals that need cells.** Bit i set means
   locally-defined variable i (0-indexed from the first local after params)
   needs cell wrapping because it's captured by a nested closure or mutated
   via `set!`. The JIT uses this to skip `LocalCell` heap allocation for
   non-captured, non-mutated `let` bindings. The VM interpreter does not
   use this mask (it cell-wraps all locals unconditionally). Both masks
   are limited to 64 entries (`u64`).

7. **Yield is a block terminator, not an instruction.** `Terminator::Yield`
   splits the block: the current block ends with yield, and a new resume block
   begins. The resume block starts with `LoadResumeValue` to capture the value
   passed to `coro/resume`.

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
| `EmptyList` | → empty_list | Push Value::EMPTY_LIST (truthy, unlike Nil) |
| `LoadResumeValue` | → value | First instruction in yield resume block |
| `CarOrNil` | value → car | Car of cons, or nil if not a cons |
| `CdrOrNil` | value → cdr | Cdr of cons, or nil if not a cons |
| `ArrayRefOrNil` | array → elem | Array element by immediate u16 index, or nil if out of bounds |
| `IsArray` | value → bool | Type check: is value an array? (for pattern matching) |
| `IsTable` | value → bool | Type check: is value a table or struct? (for pattern matching) |
| `ArrayLen` | array → int | Get array length (for pattern matching) |
| `TableGetOrNil` | table → value | Get key from table/struct, or nil if missing/wrong type (u16 const_idx operand) |

## Yield as terminator

`Terminator::Yield { value, resume_label }` correctly models that yield
suspends execution and resumes in a new block. The lowerer:

1. Emits `Terminator::Yield` to end the current block
2. Creates a new block at `resume_label`
3. Emits `LoadResumeValue` as the first instruction of the resume block

The emitter preserves stack state across the yield boundary via
`yield_stack_state`. This ensures intermediate values computed before yield
(e.g., the `1` in `(+ 1 (yield 2) 3)`) survive into the resume block.

## Block/Break lowering

`HirKind::Block` lowers to a result register + exit label pattern:
1. Allocate `result_reg` and `exit_label`
2. Push `BlockLowerContext { block_id, result_reg, exit_label }`
3. Lower body, move result to `result_reg`
4. Pop context, jump to `exit_label`, start new block at `exit_label`

`HirKind::Break` lowers to Move + Jump:
1. Find target block's `result_reg` and `exit_label` via `block_lower_contexts`
2. Lower value, move to `result_reg`, jump to `exit_label`
3. Start unreachable dead-code block

No new bytecode instructions — break compiles to existing Move + Jump.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 20 | Re-exports |
| `types.rs` | 270 | `LirFunction`, `LirInstr`, `Reg`, `Label`, etc. |
| `intrinsics.rs` | ~55 | `IntrinsicOp` enum, maps primitive SymbolIds to specialized LIR instructions (BinOp, CmpOp, UnaryOp) |
| `lower/mod.rs` | ~280 | `Lowerer` struct, context, entry point |
| `lower/expr.rs` | ~457 | Expression lowering: literals, operators, calls |
| `lower/binding.rs` | ~280 | Binding forms: `let`, `def`, `var`, `fn` |
| `lower/lambda.rs` | ~250 | fn lowering, closure capture, cell wrapping |
| `lower/control.rs` | ~200 | Control flow: `if`, `begin`, `match` |
| `lower/pattern.rs` | ~200 | Pattern matching lowering |
| `emit.rs` | 902 | `Emitter`, LIR→Bytecode with stack simulation |

## Constants

`LirConst` represents compile-time constants. Note: `LirConst::Nil` and
`LirConst::EmptyList` are distinct. Nil is falsy, EmptyList is truthy. Lists
terminate with EmptyList, not Nil.
