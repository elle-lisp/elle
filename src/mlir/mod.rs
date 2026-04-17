//! MLIR backend for Elle.
//!
//! Lowers GPU-eligible `LirFunction`s to MLIR using the melior crate,
//! then compiles through the arith/func/cf dialects to LLVM IR and
//! JIT-executes via the MLIR ExecutionEngine.

mod cache;
mod execute;
mod lower;
mod spirv;

pub use cache::MlirCache;
pub use execute::mlir_call;
pub use lower::{check_slot_types, lower_to_mlir, ScalarType};
pub use spirv::lower_to_spirv;

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

    /// Build LIR: fn(x) { if x > 0 then x else -x }  (absolute value)
    fn make_abs() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("abs".to_string());
        func.signal = Signal::errors();

        // Block 0: entry — load param, compare > 0, branch
        let mut b0 = BasicBlock::new(Label(0));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(0),
                index: 0,
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Int(0),
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Compare {
                dst: Reg(2),
                op: CmpOp::Gt,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            s(),
        ));
        b0.terminator = SpannedTerminator::new(
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
            s(),
        );

        // Block 1: then — return x
        let mut b1 = BasicBlock::new(Label(1));
        b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), s());

        // Block 2: else — return 0 - x
        let mut b2 = BasicBlock::new(Label(2));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(3),
                op: BinOp::Sub,
                lhs: Reg(1),
                rhs: Reg(0),
            },
            s(),
        ));
        b2.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());

        func.blocks = vec![b0, b1, b2];
        func.num_regs = 4;
        func
    }

    #[test]
    fn test_execute_abs_positive() {
        assert_eq!(mlir_call(&make_abs(), &[42]).unwrap(), 42);
    }

    #[test]
    fn test_execute_abs_negative() {
        assert_eq!(mlir_call(&make_abs(), &[-7]).unwrap(), 7);
    }

    #[test]
    fn test_execute_abs_zero() {
        assert_eq!(mlir_call(&make_abs(), &[0]).unwrap(), 0);
    }

    // ── Mixed-type slot rejection ─────────────────────────────────

    /// Build LIR: fn(x) { var s = 0; if x > 0 then s = 1.5 else s = 2; return s }
    /// This has a mixed-type local slot (Int in one branch, Float in another).
    fn make_mixed_type_slot() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("mixed_slot".to_string());
        func.signal = Signal::errors();
        func.num_locals = 1;

        // Block 0: entry — load param, store 0 to slot, compare, branch
        let mut b0 = BasicBlock::new(Label(0));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(0),
                index: 0,
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Int(0),
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::StoreLocal {
                slot: 0,
                src: Reg(1),
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Compare {
                dst: Reg(2),
                op: CmpOp::Gt,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            s(),
        ));
        b0.terminator = SpannedTerminator::new(
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
            s(),
        );

        // Block 1: then — store 1.5 (Float) to slot, jump to merge
        let mut b1 = BasicBlock::new(Label(1));
        b1.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(3),
                value: LirConst::Float(1.5),
            },
            s(),
        ));
        b1.instructions.push(SpannedInstr::new(
            LirInstr::StoreLocal {
                slot: 0,
                src: Reg(3),
            },
            s(),
        ));
        b1.terminator = SpannedTerminator::new(Terminator::Jump(Label(3)), s());

        // Block 2: else — store 2 (Int) to slot, jump to merge
        let mut b2 = BasicBlock::new(Label(2));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(4),
                value: LirConst::Int(2),
            },
            s(),
        ));
        b2.instructions.push(SpannedInstr::new(
            LirInstr::StoreLocal {
                slot: 0,
                src: Reg(4),
            },
            s(),
        ));
        b2.terminator = SpannedTerminator::new(Terminator::Jump(Label(3)), s());

        // Block 3: merge — load slot, return
        let mut b3 = BasicBlock::new(Label(3));
        b3.instructions.push(SpannedInstr::new(
            LirInstr::LoadLocal {
                dst: Reg(5),
                slot: 0,
            },
            s(),
        ));
        b3.terminator = SpannedTerminator::new(Terminator::Return(Reg(5)), s());

        func.blocks = vec![b0, b1, b2, b3];
        func.num_regs = 6;
        func
    }

    #[test]
    fn test_reject_mixed_type_slot() {
        let func = make_mixed_type_slot();
        // Use check_slot_types directly to avoid partially constructing
        // MLIR ops (melior cleanup of partial modules can crash).
        let err = check_slot_types(&func, 0).unwrap_err();
        assert!(
            err.contains("mixed-type local slot"),
            "should reject cross-block mixed-type slot: {}",
            err
        );
    }

    /// Build LIR: fn(x) { var s = 0; s = 1.5; return s }
    /// Sequential reassignment within a single block — should succeed.
    fn make_sequential_reassign() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("seq_reassign".to_string());
        func.signal = Signal::errors();
        func.num_locals = 1;

        let mut b0 = BasicBlock::new(Label(0));
        // Load param (unused, just for arity)
        b0.instructions.push(SpannedInstr::new(
            LirInstr::LoadCaptureRaw {
                dst: Reg(0),
                index: 0,
            },
            s(),
        ));
        // var s = 0 (Int)
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Int(0),
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::StoreLocal {
                slot: 0,
                src: Reg(1),
            },
            s(),
        ));
        // s = 1.5 (Float — same block, sequential reassignment)
        b0.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(2),
                value: LirConst::Float(1.5),
            },
            s(),
        ));
        b0.instructions.push(SpannedInstr::new(
            LirInstr::StoreLocal {
                slot: 0,
                src: Reg(2),
            },
            s(),
        ));
        // Load and return s
        b0.instructions.push(SpannedInstr::new(
            LirInstr::LoadLocal {
                dst: Reg(3),
                slot: 0,
            },
            s(),
        ));
        b0.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());

        func.blocks = vec![b0];
        func.num_regs = 4;
        func
    }

    #[test]
    fn test_accept_sequential_reassign() {
        let func = make_sequential_reassign();
        // Should lower successfully — sequential reassignment in same block is fine.
        let mlir_text = lower_to_mlir(&func).expect("sequential reassignment should succeed");
        assert!(
            mlir_text.contains("func.func"),
            "should produce valid MLIR: {}",
            mlir_text
        );
    }

    #[test]
    fn test_execute_sequential_reassign() {
        let func = make_sequential_reassign();
        let result = mlir_call(&func, &[0]).expect("execution should succeed");
        // s was reassigned from 0 (Int) to 1.5 (Float); result is f64 bits
        assert_eq!(result, 1.5f64.to_bits() as i64);
    }

    // ── SPIR-V tests ─────────────────────────────────────────────

    #[test]
    fn test_spirv_add() {
        let func = make_add();
        let spirv_bytes = lower_to_spirv(&func, 256).expect("SPIR-V lowering should succeed");
        assert!(
            spirv_bytes.len() >= 20,
            "SPIR-V should be non-trivial: {} bytes",
            spirv_bytes.len()
        );
        // SPIR-V magic number: 0x07230203
        assert_eq!(
            &spirv_bytes[0..4],
            &[0x03, 0x02, 0x23, 0x07],
            "SPIR-V magic number"
        );
    }

    #[test]
    fn test_spirv_mul_add() {
        let func = make_mul_add();
        let spirv_bytes = lower_to_spirv(&func, 64).expect("SPIR-V lowering should succeed");
        assert!(spirv_bytes.len() >= 20);
        assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    #[test]
    fn test_spirv_abs() {
        let func = make_abs();
        let spirv_bytes =
            lower_to_spirv(&func, 256).expect("multi-block SPIR-V lowering should succeed");
        assert!(spirv_bytes.len() >= 20);
        assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    /// Build LIR: fn(x) { return x + 1.5 }  (float constant + mixed promotion)
    fn make_float_add() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("float_add".to_string());
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
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Float(1.5),
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

    #[test]
    fn test_spirv_float_add() {
        let func = make_float_add();
        let spirv_bytes = lower_to_spirv(&func, 256).expect("float SPIR-V lowering should succeed");
        assert!(spirv_bytes.len() >= 20);
        assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    /// Build LIR: fn(x) { return 2.0 * 3.0 }  (pure float arithmetic)
    fn make_float_mul() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("float_mul".to_string());
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
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Float(2.0),
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(2),
                value: LirConst::Float(3.0),
            },
            s(),
        ));
        block.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(3),
                op: BinOp::Mul,
                lhs: Reg(1),
                rhs: Reg(2),
            },
            s(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), s());
        func.blocks.push(block);
        func.num_regs = 4;
        func
    }

    #[test]
    fn test_spirv_float_mul() {
        let func = make_float_mul();
        let spirv_bytes =
            lower_to_spirv(&func, 256).expect("pure-float SPIR-V lowering should succeed");
        assert!(spirv_bytes.len() >= 20);
        assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    /// Build LIR: fn(x) { return float(x) }
    fn make_int_to_float() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("int_to_float".to_string());
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
            LirInstr::Convert {
                dst: Reg(1),
                op: ConvOp::IntToFloat,
                src: Reg(0),
            },
            s(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), s());
        func.blocks.push(block);
        func.num_regs = 2;
        func
    }

    /// Build LIR: fn(x) { return int(x) }
    fn make_float_to_int() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("float_to_int".to_string());
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
            LirInstr::Convert {
                dst: Reg(1),
                op: ConvOp::FloatToInt,
                src: Reg(0),
            },
            s(),
        ));
        block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), s());
        func.blocks.push(block);
        func.num_regs = 2;
        func
    }

    #[test]
    fn test_lower_int_to_float() {
        let mlir_text = lower_to_mlir(&make_int_to_float()).expect("lowering should succeed");
        assert!(
            mlir_text.contains("arith.sitofp"),
            "should contain arith.sitofp: {}",
            mlir_text
        );
    }

    #[test]
    fn test_lower_float_to_int() {
        // Float arg via param_types bitmask
        let context = lower::create_context();
        let (module, _) = lower::lower_to_module(&context, &make_float_to_int(), 1)
            .expect("lowering should succeed");
        let mlir_text = module.as_operation().to_string();
        assert!(
            mlir_text.contains("arith.fptosi"),
            "should contain arith.fptosi: {}",
            mlir_text
        );
    }

    #[test]
    fn test_execute_int_to_float() {
        let result = mlir_call(&make_int_to_float(), &[42]).expect("execution should succeed");
        assert_eq!(result, 42.0f64.to_bits() as i64);
    }

    #[test]
    fn test_execute_float_to_int() {
        let func = make_float_to_int();
        let bits = 3.7f64.to_bits() as i64;
        // Need to call with param_types=1 to mark arg as float
        let context = lower::create_context();
        let (mut module, _) =
            lower::lower_to_module(&context, &func, 1).expect("lowering should succeed");
        let pm = melior::pass::PassManager::new(&context);
        pm.add_pass(melior::pass::conversion::create_to_llvm());
        pm.run(&mut module).expect("LLVM conversion should succeed");
        let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);
        let mut arg: i64 = bits;
        let mut result: i64 = 0;
        unsafe {
            engine
                .invoke_packed(
                    "float_to_int",
                    &mut [
                        &mut arg as *mut i64 as *mut (),
                        &mut result as *mut i64 as *mut (),
                    ],
                )
                .unwrap();
        }
        assert_eq!(result, 3, "fptosi(3.7) should be 3");
    }

    #[test]
    fn test_spirv_int_to_float() {
        let func = make_int_to_float();
        let spirv_bytes = lower_to_spirv(&func, 256).expect("SPIR-V lowering should succeed");
        assert!(spirv_bytes.len() >= 20);
        assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    #[test]
    fn bench_mlir() {
        use super::lower::{create_context, lower_to_module};
        use std::time::Instant;

        let func = make_mul_add();
        let n = 1_000_000;

        // ── MLIR: break down each phase ─────────────────────────
        let start = Instant::now();
        let context = create_context();
        let ctx_time = start.elapsed();

        let start = Instant::now();
        let (mut module, _) = lower_to_module(&context, &func, 0).unwrap();
        let lower_time = start.elapsed();

        let start = Instant::now();
        let pm = melior::pass::PassManager::new(&context);
        pm.add_pass(melior::pass::conversion::create_to_llvm());
        pm.run(&mut module).unwrap();
        let convert_time = start.elapsed();

        let start = Instant::now();
        let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);
        let jit_time = start.elapsed();

        let start = Instant::now();
        for i in 0..n {
            let mut a: i64 = i;
            let mut b: i64 = 7;
            let mut result: i64 = 0;
            unsafe {
                engine
                    .invoke_packed(
                        "mul_add",
                        &mut [
                            &mut a as *mut i64 as *mut (),
                            &mut b as *mut i64 as *mut (),
                            &mut result as *mut i64 as *mut (),
                        ],
                    )
                    .unwrap();
            }
            assert_eq!(result, i * 7 + i);
        }
        let mlir_exec_time = start.elapsed();

        // ── Cranelift: compile only (execution needs VM context) ─
        let start = Instant::now();
        let compiler = crate::jit::JitCompiler::new().unwrap();
        let cranelift_init = start.elapsed();

        let start = Instant::now();
        let _jit_code = compiler
            .compile(&func, None, std::collections::HashMap::new(), vec![])
            .unwrap();
        let cranelift_compile = start.elapsed();

        eprintln!();
        eprintln!("── mul_add(a,b) = a*b+a, {} exec iterations ──", n);
        eprintln!();
        eprintln!("  MLIR:");
        eprintln!("    context creation:  {:?}", ctx_time);
        eprintln!("    lower LIR→MLIR:    {:?}", lower_time);
        eprintln!("    convert →LLVM:     {:?}", convert_time);
        eprintln!("    LLVM JIT compile:  {:?}", jit_time);
        eprintln!(
            "    compile total:     {:?}",
            ctx_time + lower_time + convert_time + jit_time
        );
        eprintln!(
            "    exec:              {:?} ({:?}/call)",
            mlir_exec_time,
            mlir_exec_time / n as u32
        );
        eprintln!();
        eprintln!("  Cranelift:");
        eprintln!("    init:              {:?}", cranelift_init);
        eprintln!("    compile:           {:?}", cranelift_compile);
        eprintln!(
            "    compile total:     {:?}",
            cranelift_init + cranelift_compile
        );
    }
}
