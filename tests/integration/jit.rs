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
use elle::value::{Arity, Value};

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    // fn() -> true
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    // fn() -> false
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(0));
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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

    let inner_func = Box::new(LirFunction::new(Arity::Exact(0)));
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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 4;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(3));
    func.num_regs = 5;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 4;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
fn test_jit_make_array() {
    // fn(a, b, c) -> array(a, b, c)
    let mut func = LirFunction::new(Arity::Exact(3));
    func.num_regs = 4;
    func.num_captures = 0;
    func.effect = Effect::none();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(load_arg(Reg(2), 2));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeArray {
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
    assert!(result.is_array());
    let vec = result.as_array().unwrap();
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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.effect = Effect::none();

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
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Use begin to wrap multiple expressions
    let result = eval(
        r#"(begin
        (defn count-down (n)
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
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (defn sum-to (n acc)
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
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Simple test: swap args and decrement
    // Trace: (3,10) -> (10,2) -> (2,9) -> (9,1) -> (1,8) -> (8,0) -> (0,7) -> 7
    let result = eval(
        r#"(begin
        (defn swap-test (a b)
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
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (defn fib-iter (n a b)
            (if (= n 0) a (fib-iter (- n 1) b (+ a b))))
        (fib-iter 20 0 1))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "fib-iter failed: {:?}", result);
    // fib(20) = 6765
    assert_eq!(result.unwrap().as_int(), Some(6765));
}

// =============================================================================
// Fiber + JIT Gate Tests
// =============================================================================

#[test]
fn test_jit_rejects_yields_raises_effect() {
    // Effect::yields_raises() has may_suspend() = true.
    // The JIT gate must reject this — fiber/resume and fiber/signal
    // propagate this effect to their callers.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::yields_raises();

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
    assert!(
        matches!(result, Err(JitError::NotPure)),
        "JIT should reject yields_raises effect: {:?}",
        result
    );
}

#[test]
fn test_jit_accepts_raises_only_effect() {
    // Effect::raises() has may_suspend() = false.
    // The JIT gate should accept this — fiber/new, fiber/status, etc.
    // have this effect and are safe to call from JIT code.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.effect = Effect::raises();

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
    assert!(
        result.is_ok(),
        "JIT should accept raises-only effect: {:?}",
        result
    );
}

// =============================================================================
// Batch JIT: Mutual Recursion Tests
// =============================================================================

#[test]
fn test_jit_mutual_recursion_even_odd() {
    // Classic mutual recursion: is-even? and is-odd? call each other
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (var is-even? (fn (n) (if (= n 0) #t (is-odd? (- n 1)))))
        (var is-odd? (fn (n) (if (= n 0) #f (is-even? (- n 1)))))
        (list (is-even? 10) (is-odd? 10) (is-even? 11) (is-odd? 11)))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "even-odd failed: {:?}", result);
    // (is-even? 10) = #t, (is-odd? 10) = #f, (is-even? 11) = #f, (is-odd? 11) = #t
    let list = result.unwrap();
    let first = list.as_cons().unwrap();
    assert_eq!(first.first.as_bool(), Some(true)); // (is-even? 10)
    let rest1 = first.rest.as_cons().unwrap();
    assert_eq!(rest1.first.as_bool(), Some(false)); // (is-odd? 10)
    let rest2 = rest1.rest.as_cons().unwrap();
    assert_eq!(rest2.first.as_bool(), Some(false)); // (is-even? 11)
    let rest3 = rest2.rest.as_cons().unwrap();
    assert_eq!(rest3.first.as_bool(), Some(true)); // (is-odd? 11)
}

#[test]
fn test_jit_mutual_recursion_deep() {
    // Deep mutual recursion — exercises tail call optimization across SCC.
    //
    // NOTE: depth 100 is chosen deliberately. In Phase 1, direct SCC calls
    // between peers use `call + return` (not jumps), so each mutual call
    // adds a native stack frame. Deep mutual recursion (e.g., depth 2000+)
    // would segfault from native stack overflow rather than producing a
    // clean error. This is a known Phase 1 limitation — Phase 2 will
    // implement mutual tail-call elimination via function fusion.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // ping-pong: ping(n) -> pong(n-1), pong(n) -> ping(n-1)
    // Both are tail calls, so this should handle deep recursion
    let result = eval(
        r#"(begin
        (var ping (fn (n) (if (= n 0) "ping" (pong (- n 1)))))
        (var pong (fn (n) (if (= n 0) "pong" (ping (- n 1)))))
        (list (ping 0) (pong 0) (ping 1) (pong 1) (ping 100) (pong 100)))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "ping-pong failed: {:?}", result);
    let list = result.unwrap();
    let vals: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = list;
        while let Some(cons) = cur.as_cons() {
            v.push(cons.first.as_string().unwrap().to_string());
            cur = cons.rest;
        }
        v
    };
    assert_eq!(vals, vec!["ping", "pong", "pong", "ping", "ping", "pong"]);
}

#[test]
fn test_jit_mutual_recursion_nqueens_small() {
    // Verify nqueens works correctly with JIT batch compilation
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (var check-safe-helper
          (fn (col remaining row-offset)
            (if (empty? remaining)
              #t
              (let ((placed-col (first remaining)))
                (if (or (= col placed-col)
                        (= row-offset (abs (- col placed-col))))
                  #f
                  (check-safe-helper col (rest remaining) (+ row-offset 1)))))))

        (var safe?
          (fn (col queens)
            (check-safe-helper col queens 1)))

        (var try-cols-helper
          (fn (n col queens row)
            (if (= col n)
              (list)
              (if (safe? col queens)
                (let ((new-queens (cons col queens)))
                  (append (solve-helper n (+ row 1) new-queens)
                          (try-cols-helper n (+ col 1) queens row)))
                (try-cols-helper n (+ col 1) queens row)))))

        (var solve-helper
          (fn (n row queens)
            (if (= row n)
              (list (reverse queens))
              (try-cols-helper n 0 queens row))))

        (var solve-nqueens
          (fn (n)
            (solve-helper n 0 (list))))

        (length (solve-nqueens 8)))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "nqueens failed: {:?}", result);
    // 8-queens has 92 solutions
    assert_eq!(result.unwrap().as_int(), Some(92));
}

#[test]
fn test_jit_mutual_recursion_three_way() {
    // Three mutually recursive functions forming a cycle
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (var fa (fn (n) (if (= n 0) "a" (fb (- n 1)))))
        (var fb (fn (n) (if (= n 0) "b" (fc (- n 1)))))
        (var fc (fn (n) (if (= n 0) "c" (fa (- n 1)))))
        (list (fa 0) (fa 1) (fa 2) (fa 3) (fa 6) (fa 9)))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(result.is_ok(), "three-way failed: {:?}", result);
    let list = result.unwrap();
    let vals: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = list;
        while let Some(cons) = cur.as_cons() {
            v.push(cons.first.as_string().unwrap().to_string());
            cur = cons.rest;
        }
        v
    };
    // fa(0)=a, fa(1)=fb(0)=b, fa(2)=fb(1)=fc(0)=c,
    // fa(3)=fb(2)=fc(1)=fa(0)=a, fa(6)=a, fa(9)=a
    assert_eq!(vals, vec!["a", "b", "c", "a", "a", "a"]);
}

#[test]
fn test_jit_batch_global_mutation_known_limitation() {
    // Documents a known Phase 1 limitation: after batch JIT compilation,
    // mutating a global (`set!`) does NOT update the direct SCC calls.
    // The batch-compiled code still calls the old function because direct
    // calls are resolved at compilation time, not at runtime.
    //
    // This test verifies the program doesn't crash and produces *some*
    // result. The exact behavior (old vs new function) depends on whether
    // batch compilation fired for the particular call path.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Define mutually recursive functions, call them enough to trigger JIT,
    // then mutate one global and call again. The result should not crash.
    let result = eval(
        r#"(begin
        (var helper (fn (n) (if (= n 0) "original" (helper (- n 1)))))
        ;; Call enough times to trigger JIT compilation
        (helper 10)
        (helper 10)
        (helper 10)
        (helper 10)
        (helper 10)
        ;; Mutate the global
        (set! helper (fn (n) "replaced"))
        ;; Call again — may use old or new function depending on JIT state.
        ;; The key invariant: this must not crash.
        (helper 5))"#,
        &mut symbols,
        &mut vm,
    );
    assert!(
        result.is_ok(),
        "Global mutation after JIT should not crash: {:?}",
        result
    );
    // We accept either result — the point is no crash, no corruption
    let val = result.unwrap();
    assert!(
        val.as_string().is_some(),
        "Expected a string result, got: {:?}",
        val
    );
}
