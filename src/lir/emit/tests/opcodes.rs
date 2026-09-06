// audited: 2026-09-06
// docs/impl/bytecode.md
//! Which arithmetic opcode the emitter picks, and what decides it.
//!
//! An operand proof selects the integer-only bytecode; its absence selects the
//! polymorphic one. Only four operations have both.

use super::*;

/// The four arithmetic operations, paired with the opcode each spelling of the
/// operand proof selects (docs/impl/bytecode.md).
const ARITHMETIC_OPCODES: [(BinOp, &str, &str); 4] = [
    (BinOp::Add, "Add", "AddInt"),
    (BinOp::Sub, "Sub", "SubInt"),
    (BinOp::Mul, "Mul", "MulInt"),
    (BinOp::Div, "Div", "DivInt"),
];

/// `6 op 7` over two constants, built with `make_op`, disassembled to the
/// sequence of opcode tokens it emitted.
fn arithmetic_opcodes(op: BinOp, make_op: fn(Reg, BinOp, Reg, Reg) -> LirInstr) -> Vec<String> {
    use crate::compiler::bytecode::disassemble_lines;

    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(6),
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(7),
                },
                make_op(Reg(2), op, Reg(0), Reg(1)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build();

    let (bytecode, _, _) = Emitter::new().emit(&func);
    // The trap: every integer opcode name contains its polymorphic one, so a
    // substring search for "Add" also matches an emitted "AddInt". Split the
    // token off and compare it whole.
    disassemble_lines(&bytecode.instructions)
        .iter()
        .filter_map(|line| line.split_once("] "))
        .map(|(_, rest)| rest.split(' ').next().unwrap_or_default().to_string())
        .collect()
}

/// A `BinOp` carrying no operand proof can only mean the polymorphic bytecode
/// (docs/impl/bytecode.md).
#[test]
fn arithmetic_binops_emit_the_polymorphic_bytecodes() {
    // The counter-factual: without this negative half, mapping every `BinOp` to
    // its integer opcode still satisfies the positive test below, and float
    // operands would silently take integer wrapping arithmetic.
    for (op, polymorphic, integer) in ARITHMETIC_OPCODES {
        let opcodes = arithmetic_opcodes(op, LirInstr::binop);
        assert!(
            opcodes.iter().any(|name| name == polymorphic),
            "{polymorphic}: an unproven BinOp must emit the polymorphic opcode; got {opcodes:?}"
        );
        assert!(
            !opcodes.iter().any(|name| name == integer),
            "{integer}: an unproven BinOp must not emit the integer-only opcode; got {opcodes:?}"
        );
    }
}

/// A `BinOp` whose operands the front end proved are integers emits the
/// integer-only bytecode, which reads both operands without testing a tag
/// (docs/impl/lir.md).
#[test]
fn arithmetic_binops_with_an_int_proof_emit_the_integer_bytecodes() {
    for (op, polymorphic, integer) in ARITHMETIC_OPCODES {
        let opcodes = arithmetic_opcodes(op, LirInstr::int_binop);
        assert!(
            opcodes.iter().any(|name| name == integer),
            "{integer}: a proven BinOp must emit the integer-only opcode; got {opcodes:?}"
        );
        assert!(
            !opcodes.iter().any(|name| name == polymorphic),
            "{polymorphic}: a proven BinOp must not emit the polymorphic opcode; got {opcodes:?}"
        );
    }
}

/// The operations with no integer-only bytecode keep the polymorphic one
/// whatever the proof says: `Rem` has no `RemInt`, and the bitwise opcodes
/// already read their operands as integers (docs/impl/lir.md).
#[test]
fn a_proof_changes_nothing_for_an_operation_with_no_integer_opcode() {
    for op in [
        BinOp::Rem,
        BinOp::BitAnd,
        BinOp::BitOr,
        BinOp::BitXor,
        BinOp::Shl,
        BinOp::Shr,
    ] {
        assert_eq!(
            arithmetic_opcodes(op, LirInstr::int_binop),
            arithmetic_opcodes(op, LirInstr::binop),
            "{op:?}: the proof must not change an operation the instruction set \
             does not specialize"
        );
    }
}
