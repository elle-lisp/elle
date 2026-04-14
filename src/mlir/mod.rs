//! MLIR backend for Elle.
//!
//! Lowers GPU-eligible `LirFunction`s to MLIR using the melior crate,
//! then compiles through the arith/scf/func dialects to LLVM IR or SPIR-V.

mod lower;

pub use lower::lower_to_mlir;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::*;
    use crate::signals::Signal;
    use crate::syntax::Span;
    use crate::value::Arity;

    #[test]
    fn test_lower_add() {
        // Build LIR: fn(a, b) { return a + b }
        let mut func = LirFunction::new(Arity::Exact(2));
        func.signal = Signal::errors();

        let mut block = BasicBlock::new(Label(0));
        block.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(1),
                index: 1,
            },
            Span::synthetic(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(2),
                op: BinOp::Add,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            Span::synthetic(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), Span::synthetic());

        func.blocks.push(block);
        func.num_regs = 3;

        let mlir_text = lower_to_mlir(&func).expect("lowering should succeed");
        assert!(
            mlir_text.contains("arith.addi"),
            "should contain arith.addi: {}",
            mlir_text
        );
        assert!(
            mlir_text.contains("func.func"),
            "should contain func.func: {}",
            mlir_text
        );
        assert!(
            mlir_text.contains("return"),
            "should contain return: {}",
            mlir_text
        );
    }

    #[test]
    fn test_lower_constant() {
        // Build LIR: fn() { return 42 }
        let mut func = LirFunction::new(Arity::Exact(0));

        let mut block = BasicBlock::new(Label(0));
        block.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            },
            Span::synthetic(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

        func.blocks.push(block);
        func.num_regs = 1;

        let mlir_text = lower_to_mlir(&func).expect("lowering should succeed");
        assert!(
            mlir_text.contains("42"),
            "should contain constant 42: {}",
            mlir_text
        );
    }
}
