use super::*;

#[test]
fn test_bytecode_emission() {
    let mut bc = Bytecode::new();
    bc.emit(Instruction::LoadConst);
    bc.emit_u16(0);
    bc.emit(Instruction::Return);
    assert_eq!(bc.instructions.len(), 4);
}

#[test]
fn test_constant_deduplication() {
    let mut bc = Bytecode::new();
    let idx1 = bc.add_constant(Value::int(42));
    let idx2 = bc.add_constant(Value::int(42));
    assert_eq!(idx1, idx2);
    assert_eq!(bc.constants.len(), 1);
}

#[test]
fn test_instruction_roundtrip() {
    // Test that all arithmetic/bitwise instructions can be emitted and decoded
    let instructions = [
        Instruction::Add,
        Instruction::Sub,
        Instruction::Mul,
        Instruction::Div,
        Instruction::Rem,
        Instruction::BitAnd,
        Instruction::BitOr,
        Instruction::BitXor,
        Instruction::BitNot,
        Instruction::Shl,
        Instruction::Shr,
    ];

    for instr in instructions {
        let mut bc = Bytecode::new();
        bc.emit(instr);
        let byte = bc.instructions[0];
        let decoded = Instruction::from_byte(byte)
            .unwrap_or_else(|| panic!("Instruction {:?} did not decode", instr));
        assert_eq!(decoded, instr, "Instruction {:?} did not roundtrip", instr);
    }
}

#[test]
fn from_byte_rejects_out_of_range_bytes() {
    // The high-water mark is the last variant in source order; everything
    // past it must decode to None, never transmute (UB for invalid
    // discriminants).
    let last = Instruction::MaterializeConst as u8;
    assert_eq!(
        Instruction::from_byte(last),
        Some(Instruction::MaterializeConst)
    );
    assert_eq!(Instruction::from_byte(last + 1), None);
    assert_eq!(Instruction::from_byte(0xFF), None);
    assert_eq!(Instruction::from_byte(0), Some(Instruction::LoadConst));
}

#[test]
fn decref_value_region_is_operandless() {
    // DecrefValueRegion releases the value's *runtime* region (read from
    // the stack value), so it carries NO operand — the disassembler must
    // decode a following opcode immediately, not skip bytes.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::DecrefValueRegion);
    bc.emit(Instruction::Return);
    let lines = disassemble_lines(&bc.instructions);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(lines[0].contains("DecrefValueRegion"), "got: {lines:?}");
    assert!(lines[1].contains("Return"), "got: {lines:?}");
}

#[test]
fn load_self_is_operandless() {
    // LoadSelf pushes the currently-executing closure read from a runtime
    // register, so it carries NO operand — the disassembler must decode a
    // following opcode immediately, not skip bytes.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::LoadSelf);
    bc.emit(Instruction::Return);
    let lines = disassemble_lines(&bc.instructions);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(lines[0].contains("LoadSelf"), "got: {lines:?}");
    assert!(lines[1].contains("Return"), "got: {lines:?}");
    // Opcode round-trips through the byte encoding.
    let byte = bc.instructions[0];
    assert_eq!(Instruction::from_byte(byte), Some(Instruction::LoadSelf));
}

#[test]
fn adopt_into_activation_is_operandless() {
    // AdoptIntoActivation pops its child value from the operand stack and
    // carries NO operand — the disassembler must decode a following opcode
    // immediately, not skip bytes — and the opcode round-trips through the
    // byte encoding.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::AdoptIntoActivation);
    bc.emit(Instruction::Return);
    let lines = disassemble_lines(&bc.instructions);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(lines[0].contains("AdoptIntoActivation"), "got: {lines:?}");
    assert!(lines[1].contains("Return"), "got: {lines:?}");
    let byte = bc.instructions[0];
    assert_eq!(
        Instruction::from_byte(byte),
        Some(Instruction::AdoptIntoActivation)
    );
}

#[test]
fn disassemble_skips_emit_operand() {
    // Emit carries a 2-byte signal-bits operand.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::Emit);
    bc.emit_u16(0xb600);
    bc.emit(Instruction::Return);
    let lines = disassemble_lines(&bc.instructions);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(lines[0].contains("Emit"), "got: {lines:?}");
    assert!(lines[1].contains("Return"), "got: {lines:?}");
}

#[test]
fn disassemble_skips_check_signal_bound_operand() {
    // CheckSignalBound carries an 8-byte (4 × u16) operand.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::CheckSignalBound);
    bc.emit_u16(0xb600);
    bc.emit_u16(0xb601);
    bc.emit_u16(0xb602);
    bc.emit_u16(0xb603);
    bc.emit(Instruction::Return);
    let lines = disassemble_lines(&bc.instructions);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(lines[0].contains("CheckSignalBound"), "got: {lines:?}");
    assert!(lines[1].contains("Return"), "got: {lines:?}");
}

#[test]
fn disassemble_skips_tail_call_operands() {
    // TailCall carries args(u16) + region(u32) + defer_callee_release(u8) +
    // deferred_release_slot(u32) = 11 operand bytes. The disassembler must skip
    // exactly those and decode the following opcode, so the closure-cycle
    // adopt slot (0 = None) keeps the stream aligned. See
    // `LirInstr::TailCall::deferred_release_slot`.
    let mut bc = Bytecode::new();
    bc.emit(Instruction::TailCall);
    bc.emit_u16(2); // arg_count
    bc.emit_u32(7); // region slot
    bc.emit_byte(0); // defer_callee_release = false
    bc.emit_u32(9); // deferred_release_slot = Some(9)
    bc.emit(Instruction::Return);
    let lines = disassemble_lines(&bc.instructions);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(lines[0].contains("TailCall"), "got: {lines:?}");
    assert!(
        lines[0].contains("defer_callee_release=0") && lines[0].contains("deferred_release_slot=9"),
        "got: {lines:?}"
    );
    assert!(lines[1].contains("Return"), "got: {lines:?}");
}

#[test]
fn disassemble_does_not_panic_on_unknown_opcode() {
    // A byte that does not map to any Instruction variant must not
    // panic the disassembler. This guards against the
    // mem::transmute UB / panic when new opcodes are added without
    // updating the operand-size match arm.
    let bogus = [0xb6u8, 0xff];
    let lines = disassemble_lines(&bogus);
    assert_eq!(lines.len(), 2, "got: {lines:?}");
}

#[test]
fn test_bytecode_variants_distinct() {
    // Catch accidental duplication of variants (they all get auto-
    // numbered by the compiler, so any duplicate would be a compile
    // error anyway — but this test additionally guards against a
    // refactor that collapses two variants into one). All repr values
    // must be distinct; pick a few representative ones and spot-check.
    assert_ne!(
        Instruction::StructGetDestructure as u8,
        Instruction::StructGetOrNil as u8,
        "StructGetDestructure must have a distinct byte value from StructGetOrNil"
    );
    assert_ne!(
        Instruction::FirstDestructure as u8,
        Instruction::RestDestructure as u8,
    );
}
