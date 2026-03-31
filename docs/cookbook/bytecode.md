# Adding a New Bytecode Instruction


An instruction flows through three layers: definition (`compiler`),
emission (`lir`), and execution (`vm`).

### Files to modify (in order)

1. **`src/compiler/bytecode.rs`** — Add variant to `Instruction` enum.

2. **`src/compiler/bytecode_debug.rs`** — Add disassembly formatting.

3. **`src/lir/types.rs`** — Add variant to `LirInstr` enum.

4. **`src/lir/emit.rs`** — Add emission case in `emit_instr()`.

5. **`src/vm/dispatch.rs`** — Add dispatch arm in the main loop.

6. **`src/vm/<handler>.rs`** — Implement the handler function.

### Step by step

**Step 1: `src/compiler/bytecode.rs`** — Add to `Instruction` enum.
**Add at the end** (byte values are positional via `#[repr(u8)]`):

```rust
#[repr(u8)]
pub enum Instruction {
    // ... existing variants ...
    /// Description of new instruction
    MyInstr,
}
```

**Step 2: `src/compiler/bytecode_debug.rs`** — Add disassembly in
`disassemble_lines()`. If the instruction has operands, add a match arm;
otherwise the `_ => {}` catch-all handles it:

```rust
Instruction::MyInstr => {
    // If it has a u16 operand:
    if i + 1 < instructions.len() {
        let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
        line.push_str(&format!(" (index={})", idx));
        i += 2;
    }
}
```

**Step 3: `src/lir/types.rs`** — Add to `LirInstr` enum:

```rust
pub enum LirInstr {
    // ... existing ...
    /// Description
    MyInstr { dst: Reg, src: Reg },
}
```

**Step 4: `src/lir/emit.rs`** — Add emission in `emit_instr()`:

```rust
LirInstr::MyInstr { dst, src } => {
    self.ensure_on_top(*src);
    self.bytecode.emit(Instruction::MyInstr);
    self.pop();          // consumed input
    self.push_reg(*dst); // produced output
}
```

The emitter uses stack simulation. Key helpers:
- `ensure_on_top(reg)` — ensures a register's value is at stack top
- `ensure_binary_on_top(lhs, rhs)` — ensures two regs are top-2
- `push_reg(reg)` / `pop()` — track simulated stack

**Step 5: `src/vm/dispatch.rs`** — Add dispatch arm in
`execute_bytecode_inner_impl()`:

```rust
Instruction::MyInstr => {
    my_handler::handle_my_instr(self);
}
```

**Step 6: `src/vm/<handler>.rs`** — Implement the handler. Follow the
pattern in `src/vm/data.rs` or `src/vm/types.rs`:

```rust
pub fn handle_my_instr(vm: &mut VM) {
    let value = vm.fiber.stack.pop()
        .expect("VM bug: stack underflow on MyInstr");
    // ... transform value ...
    vm.fiber.stack.push(result);
}
```

### Conventions

- Stack underflow is a VM bug → `panic!` (not a user error).
- User errors → `vm.fiber.signal = Some((SIG_ERROR, error_val(...)))`,
  push `Value::NIL`, return normally.
- Instructions that consume N values and produce M values must match the
  emitter's stack simulation exactly.

---

---

## See also

- [Cookbook index](index.md)
