// JIT compilation integration tests
//
// These tests verify that the JIT compiler correctly translates LIR to native
// code and produces the same results as the interpreter.

use elle::effects::Effect;
use elle::jit::{JitCompiler, JitError};
use elle::lir::{
    BasicBlock, BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Reg, SpannedInstr,
    SpannedTerminator, Terminator, UnaryOp,
};
use elle::syntax::Span;
use elle::value::Value;

// =============================================================================
// Helper Functions
// =============================================================================

fn span() -> Span {
    Span::synthetic()
}

/// Create a LoadCapture instruction to load an argument into a register.
/// With num_captures=0, LoadCapture index N loads from args[N].
fn load_arg(dst: Reg, arg_index: u16) -> SpannedInstr {
    SpannedInstr::new(
        LirInstr::LoadCapture {
            dst,
            index: arg_index,
        },
        span(),
    )
}

fn compile_and_call(lir: &LirFunction, args: &[u64]) -> Result<Value, JitError> {
    let compiler = JitCompiler::new()?;
    let code = compiler.compile(lir)?;
    // self_bits = 0 since we're not testing self-tail-calls in these basic tests
    let result = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            args.len() as u32,
            std::ptr::null_mut(),
            0,
        )
    };
    Ok(unsafe { Value::from_bits(result) })
}

// =============================================================================
// Basic Tests
// =============================================================================

#[test]
fn test_jit_identity() {
    // fn(x) -> x
    let mut func = LirFunction::new(1);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_constant() {
    // fn() -> 42
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_nil() {
    // fn() -> nil
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Nil,
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert!(result.is_nil());
}

#[test]
fn test_jit_bool_true() {
    // fn() -> #t
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Bool(true),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_bool_false() {
    // fn() -> #f
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Bool(false),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_empty_list() {
    // fn() -> ()
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::EmptyList,
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert!(result.is_empty_list());
}

// =============================================================================
// Arithmetic Tests
// =============================================================================

#[test]
fn test_jit_add() {
    // fn(x, y) -> x + y
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(10).to_bits(), Value::int(32).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_sub() {
    // fn(x, y) -> x - y
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Sub,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(50).to_bits(), Value::int(8).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_mul() {
    // fn(x, y) -> x * y
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Mul,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(6).to_bits(), Value::int(7).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_div() {
    // fn(x, y) -> x / y
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Div,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(84).to_bits(), Value::int(2).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_rem() {
    // fn(x, y) -> x % y
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Rem,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(47).to_bits(), Value::int(5).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_jit_neg() {
    // fn(x) -> -x
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::UnaryOp {
            dst: Reg(1),
            op: UnaryOp::Neg,
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(-42));
}

// =============================================================================
// Comparison Tests
// =============================================================================

#[test]
fn test_jit_lt_true() {
    // fn(x, y) -> x < y
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Lt,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(1).to_bits(), Value::int(2).to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_lt_false() {
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Lt,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(2).to_bits(), Value::int(1).to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_eq() {
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Eq,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(42).to_bits(), Value::int(42).to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    let result2 =
        compile_and_call(&func, &[Value::int(42).to_bits(), Value::int(43).to_bits()]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}

// =============================================================================
// Control Flow Tests
// =============================================================================

#[test]
fn test_jit_branch_true() {
    // fn(x) -> if x then 1 else 0
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    // Entry block: load arg, branch on x
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    // Then block: return 1
    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    // Else block: return 0
    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    // Test with true
    let result = compile_and_call(&func, &[Value::TRUE.to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_jit_branch_false() {
    // fn(x) -> if x then 1 else 0
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    // Test with false
    let result = compile_and_call(&func, &[Value::FALSE.to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_nil() {
    // nil is falsy
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::NIL.to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_integer_truthy() {
    // Non-zero integers are truthy
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_jit_rejects_yielding() {
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::yields();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func);
    assert!(matches!(result, Err(JitError::NotPure)));
}

#[test]
fn test_jit_call_compiles() {
    // Test that Call instruction compiles (Phase 3)
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Call {
            dst: Reg(1),
            func: Reg(0),
            args: vec![],
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func);
    // Call should now compile successfully
    assert!(result.is_ok(), "Call should compile: {:?}", result);
}

#[test]
fn test_jit_rejects_make_closure() {
    // MakeClosure is still unsupported (Phase 4+)
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let inner_func = Box::new(LirFunction::new(0));
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeClosure {
            dst: Reg(0),
            func: inner_func,
            captures: vec![],
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func);
    assert!(matches!(result, Err(JitError::UnsupportedInstruction(_))));
}

// =============================================================================
// Complex Expression Tests
// =============================================================================

#[test]
fn test_jit_conditional_arithmetic() {
    // fn(x) -> if (x = 0) then 1 else (x * 2)
    let mut func = LirFunction::new(1);
    func.num_regs = 4;
    func.num_captures = 0;
    func.effect = Effect::pure();

    // Entry: load arg, compare x == 0
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Eq,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(2),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    // Then: return 1
    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(3),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());

    // Else: return x * 2
    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(2),
        },
        span(),
    ));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(3),
            op: BinOp::Mul,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    // Test x = 0 -> 1
    let result = compile_and_call(&func, &[Value::int(0).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(1));

    // Test x = 5 -> 10
    let result2 = compile_and_call(&func, &[Value::int(5).to_bits()]).unwrap();
    assert_eq!(result2.as_int(), Some(10));
}

#[test]
fn test_jit_chained_arithmetic() {
    // fn(a, b, c) -> (a + b) * c
    let mut func = LirFunction::new(3);
    func.num_regs = 5;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(load_arg(Reg(2), 2));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(3),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(4),
            op: BinOp::Mul,
            lhs: Reg(3),
            rhs: Reg(2),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(4)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // (2 + 5) * 6 = 42
    let result = compile_and_call(
        &func,
        &[
            Value::int(2).to_bits(),
            Value::int(5).to_bits(),
            Value::int(6).to_bits(),
        ],
    )
    .unwrap();
    assert_eq!(result.as_int(), Some(42));
}

// =============================================================================
// Bitwise Operation Tests
// =============================================================================

#[test]
fn test_jit_bit_and() {
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::BitAnd,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // 0b1111 & 0b1010 = 0b1010 = 10
    let result =
        compile_and_call(&func, &[Value::int(15).to_bits(), Value::int(10).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(10));
}

#[test]
fn test_jit_bit_or() {
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::BitOr,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // 0b1100 | 0b0011 = 0b1111 = 15
    let result =
        compile_and_call(&func, &[Value::int(12).to_bits(), Value::int(3).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_jit_shl() {
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Shl,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // 1 << 4 = 16
    let result =
        compile_and_call(&func, &[Value::int(1).to_bits(), Value::int(4).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(16));
}

// =============================================================================
// Logical Operation Tests
// =============================================================================

#[test]
fn test_jit_not_true() {
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::UnaryOp {
            dst: Reg(1),
            op: UnaryOp::Not,
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::TRUE.to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_not_false() {
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::UnaryOp {
            dst: Reg(1),
            op: UnaryOp::Not,
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::FALSE.to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_not_nil() {
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::UnaryOp {
            dst: Reg(1),
            op: UnaryOp::Not,
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::NIL.to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

// =============================================================================
// Float Tests
// =============================================================================

#[test]
fn test_jit_float_constant() {
    let mut func = LirFunction::new(0);
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Float(1.234),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert!((result.as_float().unwrap() - 1.234).abs() < 0.001);
}

#[test]
fn test_jit_float_add() {
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(
        &func,
        &[Value::float(1.5).to_bits(), Value::float(2.5).to_bits()],
    )
    .unwrap();
    assert!((result.as_float().unwrap() - 4.0).abs() < 0.001);
}

// =============================================================================
// Phase 3: Data Structure Tests
// =============================================================================

#[test]
fn test_jit_cons() {
    // fn(x, y) -> cons(x, y)
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Cons {
            dst: Reg(2),
            head: Reg(0),
            tail: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result =
        compile_and_call(&func, &[Value::int(1).to_bits(), Value::int(2).to_bits()]).unwrap();
    assert!(result.is_cons());
    let cons = result.as_cons().unwrap();
    assert_eq!(cons.first.as_int(), Some(1));
    assert_eq!(cons.rest.as_int(), Some(2));
}

#[test]
fn test_jit_car_cdr() {
    // fn(pair) -> car(pair) + cdr(pair)
    // Assumes pair is (a . b) where a and b are integers
    let mut func = LirFunction::new(1);
    func.num_regs = 4;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Car {
            dst: Reg(1),
            pair: Reg(0),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Cdr {
            dst: Reg(2),
            pair: Reg(0),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(3),
            op: BinOp::Add,
            lhs: Reg(1),
            rhs: Reg(2),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // Create a cons cell (10 . 32)
    let pair = Value::cons(Value::int(10), Value::int(32));
    let result = compile_and_call(&func, &[pair.to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_is_pair() {
    // fn(x) -> is_pair(x)
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::IsPair {
            dst: Reg(1),
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // Test with a cons cell
    let pair = Value::cons(Value::int(1), Value::int(2));
    let result = compile_and_call(&func, &[pair.to_bits()]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    // Test with an integer
    let result2 = compile_and_call(&func, &[Value::int(42).to_bits()]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}

#[test]
fn test_jit_make_vector() {
    // fn(a, b, c) -> vector(a, b, c)
    let mut func = LirFunction::new(3);
    func.num_regs = 4;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(load_arg(Reg(2), 2));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeVector {
            dst: Reg(3),
            elements: vec![Reg(0), Reg(1), Reg(2)],
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(
        &func,
        &[
            Value::int(1).to_bits(),
            Value::int(2).to_bits(),
            Value::int(3).to_bits(),
        ],
    )
    .unwrap();
    assert!(result.is_vector());
    let vec = result.as_vector().unwrap();
    let borrowed = vec.borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0].as_int(), Some(1));
    assert_eq!(borrowed[1].as_int(), Some(2));
    assert_eq!(borrowed[2].as_int(), Some(3));
}

// =============================================================================
// Phase 3: Cell Tests
// =============================================================================

#[test]
fn test_jit_make_cell() {
    // fn(x) -> make_cell(x)
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeCell {
            dst: Reg(1),
            value: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42).to_bits()]).unwrap();
    assert!(result.is_local_cell());
    let cell = result.as_cell().unwrap();
    assert_eq!(cell.borrow().as_int(), Some(42));
}

#[test]
fn test_jit_load_cell() {
    // fn(cell) -> load_cell(cell)
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCell {
            dst: Reg(1),
            cell: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let cell = Value::local_cell(Value::int(42));
    let result = compile_and_call(&func, &[cell.to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_store_cell() {
    // fn(cell, value) -> store_cell(cell, value); load_cell(cell)
    let mut func = LirFunction::new(2);
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0)); // cell
    entry.instructions.push(load_arg(Reg(1), 1)); // value
    entry.instructions.push(SpannedInstr::new(
        LirInstr::StoreCell {
            cell: Reg(0),
            value: Reg(1),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCell {
            dst: Reg(2),
            cell: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let cell = Value::local_cell(Value::int(0));
    let result = compile_and_call(&func, &[cell.to_bits(), Value::int(42).to_bits()]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

// =============================================================================
// TailCall Tests
// =============================================================================

#[test]
fn test_jit_tail_call_compiles() {
    // TailCall should now compile (not return UnsupportedInstruction)
    // Build a simple function: fn(x) -> tail_call(x)
    let mut func = LirFunction::new(1);
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::pure();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::TailCall {
            func: Reg(0),
            args: vec![],
        },
        span(),
    ));
    // TailCall emits a return, so we need Unreachable as the terminator
    entry.terminator = SpannedTerminator::new(Terminator::Unreachable, span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func);
    // TailCall should now compile successfully
    assert!(result.is_ok(), "TailCall should compile: {:?}", result);
}

// =============================================================================
// Self-Tail-Call Optimization Tests (End-to-End)
// =============================================================================

#[test]
fn test_jit_self_tail_call_loop() {
    // This should compile to a native loop, not bounce to interpreter
    use elle::pipeline::eval_new;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Use begin to wrap multiple expressions
    let result = eval_new(
        r#"(begin
        (define (count-down n)
            (if (= n 0) 0 (count-down (- n 1))))
        (count-down 100000))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "count-down failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(0));
}

#[test]
fn test_jit_self_tail_call_accumulator() {
    use elle::pipeline::eval_new;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval_new(
        r#"(begin
        (define (sum-to n acc)
            (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
        (sum-to 10000 0))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "sum-to failed: {:?}", result);
    // sum 1..10000 = 50005000
    let val = result.unwrap();
    assert_eq!(val.as_int(), Some(50005000));
}

#[test]
fn test_jit_self_tail_call_with_swapped_args() {
    // Test that self-tail-calls correctly handle argument swapping
    // e.g., (f b a) where args are swapped
    use elle::pipeline::eval_new;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Simple test: swap args and decrement
    // Trace: (3,10) -> (10,2) -> (2,9) -> (9,1) -> (1,8) -> (8,0) -> (0,7) -> 7
    let result = eval_new(
        r#"(begin
        (define (swap-test a b)
            (if (= a 0) b (swap-test b (- a 1))))
        (swap-test 3 10))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "swap-test failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(7));
}

#[test]
fn test_jit_self_tail_call_fibonacci_iterative() {
    // Iterative fibonacci using tail recursion
    use elle::pipeline::eval_new;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval_new(
        r#"(begin
        (define (fib-iter n a b)
            (if (= n 0) a (fib-iter (- n 1) b (+ a b))))
        (fib-iter 20 0 1))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "fib-iter failed: {:?}", result);
    // fib(20) = 6765
    assert_eq!(result.unwrap().as_int(), Some(6765));
}
