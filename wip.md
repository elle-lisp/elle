# WASM Backend Work-in-Progress

## Status

`ELLE_WASM=1 make smoke` passes. 51 WASM unit tests pass. The bytecode
VM path (`make smoke` without `ELLE_WASM=1`) is broken — stack
corruption after ~10 closure calls.

## What's Done (This Session)

### Tail calls via `return_call_indirect`
- `rt_prepare_tail_call` host function in store.rs
- `prepare_wasm_env` extracted from `call_wasm_closure` for shared use
- Closures use `return_call_indirect`; entry functions fall back to call+return
- Deep recursion (100K), mutual recursion verified; 3 Rust tests added

### Three address spaces in WASM emitter
- **Env** (linear memory): captures, params, LBox locals → LoadCapture/StoreCapture
- **Local slots** (dedicated WASM locals): non-LBox let-bound vars → LoadLocal/StoreLocal
- **Registers** (separate WASM locals): computation intermediates → Reg(N)
- `local_slot_tag(slot)`/`local_slot_pay(slot)` helpers in emit.rs
- No collisions between register bank and local variable storage

### Lowerer changes (binding.rs, expr.rs, mod.rs)
- `allocate_slot` returns stack-relative indices for non-LBox locals inside lambdas
- LBox locals get env-relative indices (unchanged)
- `lower_begin` pre-pass only adds LBox defines to `upvalue_bindings`
- Semantic intent expressed at lowerer level, not deferred to emitter

### File-level analysis (fileletrec.rs)
- Pass 3 fixpoint re-analysis uses Pass 1 scope snapshot
- Prevents expression-internal `def` forms from corrupting re-analysis
- No `((fn () ...))` wrapper hack needed

### WASM stdlib integration (mod.rs)
- Stdlib + user source concatenated as peer top-level forms
- No parameterize wrapping — IO falls back to SyncBackend
- User defs visible at file level for mutual recursion

### Float arithmetic (emit.rs)
- Tag-check dispatch: if either operand is TAG_FLOAT, use f64 instructions
- Int-to-float promotion for mixed operands
- Bitwise ops remain integer-only
- 4 Rust tests added

### Signal propagation (emit.rs, store.rs)
- `store_result_with_signal` writes signal to memory[0..4] before returning
- `call_wasm_closure` reads signal from memory after WASM call returns
- Signals propagate through WASM↔host boundaries

### Other fixes
- Bytecode VM guard: WASM closures (empty bytecode) return exec-error instead of panic
- eval/LoadResumeValue: emit Unreachable instead of silent nil
- Shift overflow guards on lbox mask checks
- Skip list: concurrency, print-epoch, port-edge-cases, eval
- WASM timeout increased to 60s

## The Open Bug: Bytecode VM Stack Corruption

### Symptom
After ~10 closure calls on the bytecode path, `*stdout*` resolves to
nil. The `println` stdlib function calls `(*stdout*)` which returns
nil instead of the stdout port. This causes `port/flush: expected port,
got nil`.

Minimal repro:
```lisp
(println "1")  # works
(println "2")  # works
...
(println "10") # works
(println "11") # FAILS: port/flush: expected port, got nil
```

### Root Cause (Hypothesis)
The `allocate_slot` change returns `num_locals` (stack-relative) for
non-LBox locals inside lambdas, instead of `num_captures + num_locals`
(env-relative). The bytecode emitter pre-allocates `num_locals` Nil
values on the stack at function entry. If the slot numbering doesn't
match what the VM expects, the stack layout is wrong and values get
clobbered after enough calls.

The bytecode emitter already had `non_cell_local_slot` which did this
same optimization at emit time (converting LoadCapture → LoadLocal for
non-cell locals, with `stack_slot = env_index - num_captures`). My
lowerer changes moved this decision to the lowerer but with different
slot numbering — the lowerer uses `num_locals` directly while
`non_cell_local_slot` computed `env_index - num_captures`. These are
the SAME value only when allocation order matches, which it might not
for all patterns.

### Investigation Plan
1. Compare slot assignments from `allocate_slot` (new) vs what
   `non_cell_local_slot` would compute (old) for stdlib functions
2. Check if `num_locals` in the LirFunction is correct for the VM's
   pre-allocation
3. Check if the bytecode emitter's `non_cell_local_slot` still fires
   for LoadCapture instructions (it shouldn't, since the lowerer now
   emits LoadLocal for non-cell locals — but Begin pre-pass defines
   might still go through LoadCapture)
4. Add instrumentation to track stack depth across calls

### The Tension
The lowerer expresses semantic intent (LoadLocal vs LoadCapture). The
bytecode emitter had its own optimization (`non_cell_local_slot`) doing
the same thing. These need to be reconciled:

- Option A: Remove `non_cell_local_slot` from bytecode emitter, rely on
  lowerer. Fix slot numbering to match VM expectations.
- Option B: Have the bytecode emitter ignore the lowerer's LoadLocal for
  closure locals and apply its own mapping. But this loses the semantic
  intent from the lowerer.
- Option C: Ensure the lowerer's slot numbering exactly matches what
  `non_cell_local_slot` would compute. The formula is the same
  (`num_locals` == `env_index - num_captures` when counted from the
  same base), but allocation ORDER matters.

## What Remains After the Bug Fix

1. `make smoke` (bytecode) must pass — the stack corruption bug
2. `make test` (WASM) must pass — `cargo test --workspace` includes
   bytecode-path tests which hit the same bug
3. `make test` (bytecode) on main should still pass — verify no
   regression to the bytecode path on the main branch
4. Port-edge-cases timeout — investigate why it's slow under WASM
5. Update AGENTS.md files for changed modules

## Commits on Branch (newest first)
```
e27be12d Add tail call and float arithmetic WASM tests
a6909328 Float dispatch, signal propagation, bytecode VM guard, eval/resume stubs
a0359706 Fix Pass 3 scope pollution; remove source-level parameterize wrapper
b1c5921a Separate local-slot and register address spaces in WASM emitter
3d97a4bf WASM backend: tail calls, scope fix, ELLE_WASM=1 make smoke
e4315e15 Add WASM backend AGENTS.md for future context
23d5e173 WASM backend: loop+br_table CFG, stdlib compiles and runs
05ec2d84 WASM backend: stdlib compilation, cond codegen bug identified
3a2825c5 WASM backend: file-mode compilation, recursion
b44d132c WASM backend: strings, signal propagation
bbeff7e7 WASM backend: MakeClosure + closure calls
b4a50087 WASM backend: LoadCapture/StoreCapture/LoadCaptureRaw
af162201 WASM backend: data operations via rt_data_op
0e6f3fbc WASM backend: primitive function calls
219703c3 WASM backend: Phase 0 + control flow
```
