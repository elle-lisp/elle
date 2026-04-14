//! MLIR backend for Elle.
//!
//! Lowers GPU-eligible `LirFunction`s to MLIR using the melior crate,
//! then compiles through the arith/func/cf dialects to LLVM IR and
//! JIT-executes via the MLIR ExecutionEngine.

mod execute;
mod lower;

pub use execute::mlir_call;
pub use lower::lower_to_mlir;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::*;
    use crate::signals::Signal;
    use crate::syntax::Span;
    use crate::value::Arity;

    fn s() -> Span {
        Span::synthetic()
    }

    /// Build LIR: fn(a, b) { return a + b }
    fn make_add() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(2));
        func.name = Some("add".to_string());
        func.signal = Signal::errors();
        let mut block = BasicBlock::new(Label(0));
        block.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(0),
                index: 0,
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(1),
                index: 1,
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(2),
                op: BinOp::Add,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            s(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), s());
        func.blocks.push(block);
        func.num_regs = 3;
        func
    }

    /// Build LIR: fn() { return 42 }
    fn make_const() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(0));
        func.name = Some("the_answer".to_string());
        let mut block = BasicBlock::new(Label(0));
        block.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            },
            s(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), s());
        func.blocks.push(block);
        func.num_regs = 1;
        func
    }

    /// Build LIR: fn(a, b) { return a * b + a }
    fn make_mul_add() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(2));
        func.name = Some("mul_add".to_string());
        func.signal = Signal::errors();
        let mut block = BasicBlock::new(Label(0));
        block.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(0),
                index: 0,
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(1),
                index: 1,
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(2),
                op: BinOp::Mul,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(3),
                op: BinOp::Add,
                lhs: Reg(2),
                rhs: Reg(0),
            },
            s(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());
        func.blocks.push(block);
        func.num_regs = 4;
        func
    }

    // ── Lowering tests ──────────────────────────────────────────────

    #[test]
    fn test_lower_add() {
        let mlir_text = lower_to_mlir(&make_add()).expect("lowering should succeed");
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
    }

    #[test]
    fn test_lower_constant() {
        let mlir_text = lower_to_mlir(&make_const()).expect("lowering should succeed");
        assert!(
            mlir_text.contains("42"),
            "should contain constant 42: {}",
            mlir_text
        );
    }

    // ── Execution tests ─────────────────────────────────────────────

    #[test]
    fn test_execute_constant() {
        let result = mlir_call(&make_const(), &[]).expect("execution should succeed");
        assert_eq!(result, 42);
    }

    #[test]
    fn test_execute_add() {
        let result = mlir_call(&make_add(), &[10, 32]).expect("execution should succeed");
        assert_eq!(result, 42);
    }

    #[test]
    fn test_execute_add_negative() {
        let result = mlir_call(&make_add(), &[-5, 15]).expect("execution should succeed");
        assert_eq!(result, 10);
    }

    #[test]
    fn test_execute_mul_add() {
        // a * b + a = 3 * 7 + 3 = 24
        let result = mlir_call(&make_mul_add(), &[3, 7]).expect("execution should succeed");
        assert_eq!(result, 24);
    }
}
