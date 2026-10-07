# Adding a New Bytecode Instruction

<!-- audited: 2026-10-06 -->

An instruction runs through four layers: its opcode, the LIR instruction that carries it, the emission between them, and the VM handler.

### Files to modify (in order)

1. [src/compiler/bytecode/instruction.rs](../../src/compiler/bytecode/instruction.rs)
   — add a variant to `Instruction`, ahead of `MaterializeConst`.
2. [src/compiler/bytecode/disasm.rs](../../src/compiler/bytecode/disasm.rs)
   — give `disassemble_lines` an arm that reads the variant's operands.
3. [src/lir/code/instr.rs](../../src/lir/code/instr.rs) — add a variant to
   `InstrRef`, with its doc comment, and add its name to the `ops!` list in
   [src/lir/code/op.rs](../../src/lir/code/op.rs).
4. [src/lir/build/](../../src/lir/build/mod.rs) — encode the variant into a
   node, and [src/lir/code/decode.rs](../../src/lir/code/decode.rs) — decode
   it back.
5. [src/lir/display.rs](../../src/lir/display.rs) — the compact display.
6. [src/lir/emit/instr.rs](../../src/lir/emit/instr.rs) — emit the bytecode
   from `emit_instr`, or from the `destructure` and `ops` files it hands to.
7. [src/vm/dispatch/interp/opcodes.rs](../../src/vm/dispatch/interp/opcodes.rs)
   — route the opcode to its handler in `dispatch_instruction`.
8. The handler, in the VM module for its family: `src/vm/data.rs`,
   `src/vm/types.rs` and their neighbours.

Then give the lowerer a site that emits the instruction, and give each backend
that translates LIR — the JIT, WASM and MLIR — a translation or a refusal.

### Step by step

**Step 1: the opcode.** `Instruction` is `#[repr(u8)]` with no explicit
discriminants, so its byte values are positional. `Instruction::from_byte`
accepts every byte up to `MaterializeConst`'s, so `MaterializeConst` stays the
last variant. Add the new one just before it:

```rust
#[repr(u8)]
pub enum Instruction {
    // ... existing variants ...
    /// Description of the new instruction
    MyInstr,
    /// ... (must remain the last variant)
    MaterializeConst,
}
```

**Step 2: the disassembler.** An opcode with no operands needs nothing: the
`_ => {}` arm takes it. An opcode with operands needs an arm that reads them
and advances `i` past them, or every later offset in the listing is wrong:

```rust
Instruction::MyInstr if i + 1 < instructions.len() => {
    let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
    line.push_str(&format!(" (index={})", idx));
    i += 2;
}
```

**Step 3: the LIR instruction.** The variant's doc comment is where a reader
of LIR learns what it does. The opcode list in `op.rs` is one macro, so `Op`
and `Op::ALL` cannot disagree. `exemplar` in
[src/lir/code/tests/mod.rs](../../src/lir/code/tests/mod.rs) matches every
opcode, so the compiler asks for a sample of the new one; the round-trip
tests beside it fail until the encoder and the decoder agree on it.

**Step 4: emission.** The emitter simulates the operand stack:

```rust
InstrRef::MyInstr { dst, src } => {
    self.ensure_on_top(*src);
    self.bytecode.emit(Instruction::MyInstr);
    self.pop();          // consumed input
    self.push_reg(*dst); // produced output
}
```

- `ensure_on_top(reg)` — puts a register's value on top of the stack
- `push_reg(reg)` / `pop()` — track the simulated stack

**Step 5: dispatch.** An opcode whose whole effect is on the operand stack —
it pops its operands and pushes a result, reading no bytecode — joins the arm
that hands to `dispatch_scalar`
([scalar.rs](../../src/vm/dispatch/interp/scalar.rs)). Any other takes an arm
of its own:

```rust
Instruction::MyInstr => {
    data::handle_my_instr(self);
}
```

**Step 6: the handler.**

```rust
pub fn handle_my_instr(vm: &mut VM) {
    let value = vm.fiber.stack.pop()
        .expect("VM bug: stack underflow on MyInstr");
    // ... compute the result ...
    vm.fiber.stack.push(result);
}
```

### Conventions

- Stack underflow is a VM bug → `panic!`, not a user error.
- A user error sets `vm.fiber.signal` to `(SIG_ERROR, error)`, with the error
  built by one of the VM's constructors such as `escaping_error`, and pushes
  `nil` in place of the result. The dispatch loop takes that placeholder off
  ([impl/vm.md](../impl/vm.md) § "The error exit").
- An instruction that consumes N values and produces M values must match the
  emitter's stack simulation exactly.

---

## See also

- [Cookbook index](index.md)
- [impl/bytecode.md](../impl/bytecode.md) — the instruction set
- [impl/lir.md](../impl/lir.md) — the LIR's two forms
