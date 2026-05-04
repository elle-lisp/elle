// JIT compilation integration tests
//
// These tests verify that the JIT compiler correctly translates LIR to native
// code and produces the same results as the interpreter.

use elle::jit::{JitCompiler, JitError};
use elle::lir::{
    BasicBlock, BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Reg, SpannedInstr,
    SpannedTerminator, Terminator, UnaryOp,
};
use elle::signals::Signal;
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

fn compile_and_call(lir: &LirFunction, args: &[Value]) -> Result<Value, JitError> {
    let compiler = JitCompiler::new()?;
    let code = compiler.compile(lir, None, std::collections::HashMap::new(), Vec::new())?;
    // self_tag/self_payload = 0 since we're not testing self-tail-calls in these basic tests
    let result = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            args.len() as u32,
            std::ptr::null_mut(),
            0,
            0,
        )
    };
    Ok(result.to_value())
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
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_constant() {
    // fn() -> 42
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

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
    func.signal = Signal::silent();

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
    func.signal = Signal::silent();

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
    func.signal = Signal::silent();

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
    func.signal = Signal::silent();

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
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(10), Value::int(32)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_sub() {
    // fn(x, y) -> x - y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(50), Value::int(8)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_mul() {
    // fn(x, y) -> x * y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(6), Value::int(7)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_div() {
    // fn(x, y) -> x / y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(84), Value::int(2)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_rem() {
    // fn(x, y) -> x % y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(47), Value::int(5)]).unwrap();
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_jit_neg() {
    // fn(x) -> -x
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
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
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(1), Value::int(2)]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_lt_false() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(2), Value::int(1)]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_eq() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(42), Value::int(42)]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    let result2 = compile_and_call(&func, &[Value::int(42), Value::int(43)]).unwrap();
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
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::TRUE]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_jit_branch_false() {
    // fn(x) -> if x then 1 else 0
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::FALSE]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_nil() {
    // nil is falsy
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::NIL]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_integer_truthy() {
    // Non-zero integers are truthy
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_jit_accepts_yielding() {
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::yields();

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
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept yielding functions via side-exit: {:?}",
        result
    );
}

#[test]
fn test_jit_call_compiles() {
    // Test that Call instruction compiles (Phase 3)
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    // Call should now compile successfully
    assert!(result.is_ok(), "Call should compile: {:?}", result);
}

#[test]
fn test_jit_rejects_make_closure() {
    // MakeClosure is rejected at the gate — the per-compilation cost of
    // emitting module closures' bytecodes is too high. Functions with
    // MakeClosure fall back to the interpreter.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeClosure {
            dst: Reg(0),
            closure_id: elle::lir::ClosureId(0),
            captures: vec![],
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(
        &func,
        None,
        std::collections::HashMap::new(),
        Vec::new(),
    );
    assert!(
        matches!(result, Err(elle::jit::JitError::UnsupportedInstruction(_))),
        "MakeClosure should be rejected: {:?}",
        result,
    );
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
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result.as_int(), Some(1));

    // Test x = 5 -> 10
    let result2 = compile_and_call(&func, &[Value::int(5)]).unwrap();
    assert_eq!(result2.as_int(), Some(10));
}

#[test]
fn test_jit_chained_arithmetic() {
    // fn(a, b, c) -> (a + b) * c
    let mut func = LirFunction::new(Arity::Exact(3));
    func.num_regs = 5;
    func.num_captures = 0;
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::int(2), Value::int(5), Value::int(6)]).unwrap();
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
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::int(15), Value::int(10)]).unwrap();
    assert_eq!(result.as_int(), Some(10));
}

#[test]
fn test_jit_bit_or() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::int(12), Value::int(3)]).unwrap();
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_jit_shl() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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
    let result = compile_and_call(&func, &[Value::int(1), Value::int(4)]).unwrap();
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
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::TRUE]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_not_false() {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::FALSE]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_not_nil() {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::NIL]).unwrap();
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
    func.signal = Signal::silent();

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
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::float(1.5), Value::float(2.5)]).unwrap();
    assert!((result.as_float().unwrap() - 4.0).abs() < 0.001);
}

// =============================================================================
// Phase 3: Data Structure Tests
// =============================================================================

#[test]
fn test_jit_cons() {
    // fn(x, y) -> pair(x, y)
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::List {
            dst: Reg(2),
            head: Reg(0),
            tail: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(1), Value::int(2)]).unwrap();
    assert!(result.is_pair());
    let pair = result.as_pair().unwrap();
    assert_eq!(pair.first.as_int(), Some(1));
    assert_eq!(pair.rest.as_int(), Some(2));
}

#[test]
fn test_jit_car_cdr() {
    // fn(pair) -> first(pair) + rest(pair)
    // Assumes pair is (a . b) where a and b are integers
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 4;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::First {
            dst: Reg(1),
            pair: Reg(0),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Rest {
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

    // Create a pair cell (10 . 32)
    let pair = Value::pair(Value::int(10), Value::int(32));
    let result = compile_and_call(&func, &[pair]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_is_pair() {
    // fn(x) -> is_pair(x)
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    // Test with a pair cell
    let pair = Value::pair(Value::int(1), Value::int(2));
    let result = compile_and_call(&func, &[pair]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    // Test with an integer
    let result2 = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}

#[test]
fn test_jit_make_array() {
    // fn(a, b, c) -> array(a, b, c)
    let mut func = LirFunction::new(Arity::Exact(3));
    func.num_regs = 4;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(load_arg(Reg(2), 2));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeArrayMut {
            dst: Reg(3),
            elements: vec![Reg(0), Reg(1), Reg(2)],
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(1), Value::int(2), Value::int(3)]).unwrap();
    assert!(result.is_array_mut());
    let vec = result.as_array_mut().unwrap();
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
fn test_jit_make_lbox() {
    // fn(x) -> make_lbox(x)
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeCaptureCell {
            dst: Reg(1),
            value: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert!(result.is_capture_cell());
    let cell = result.as_capture_cell().unwrap();
    assert_eq!(cell.borrow().as_int(), Some(42));
}

#[test]
fn test_jit_load_lbox() {
    // fn(cell) -> load_lbox(cell)
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCaptureCell {
            dst: Reg(1),
            cell: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let cell = Value::capture_cell(Value::int(42));
    let result = compile_and_call(&func, &[cell]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_store_lbox() {
    // fn(cell, value) -> store_lbox(cell, value); load_lbox(cell)
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0)); // cell
    entry.instructions.push(load_arg(Reg(1), 1)); // value
    entry.instructions.push(SpannedInstr::new(
        LirInstr::StoreCaptureCell {
            cell: Reg(0),
            value: Reg(1),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCaptureCell {
            dst: Reg(2),
            cell: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let cell = Value::capture_cell(Value::int(0));
    let result = compile_and_call(&func, &[cell, Value::int(42)]).unwrap();
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
    func.signal = Signal::silent();

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
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    // Use begin to wrap multiple expressions
    let result = eval(
        r#"(begin
        (defn count-down (n)
            (if (%eq n 0) 0 (count-down (%sub n 1))))
        (count-down 100000))"#,
        &mut symbols,
        &mut vm,
        "<test>",
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (defn sum-to (n acc)
            (if (%eq n 0) acc (sum-to (%sub n 1) (%add acc n))))
        (sum-to 10000 0))"#,
        &mut symbols,
        &mut vm,
        "<test>",
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    // Simple test: swap args and decrement
    // Trace: (3,10) -> (10,2) -> (2,9) -> (9,1) -> (1,8) -> (8,0) -> (0,7) -> 7
    let result = eval(
        r#"(begin
        (defn swap-test (a b)
            (if (%eq a 0) b (swap-test b (%sub a 1))))
        (swap-test 3 10))"#,
        &mut symbols,
        &mut vm,
        "<test>",
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (defn fib-iter (n a b)
            (if (%eq n 0) a (fib-iter (%sub n 1) b (%add a b))))
        (fib-iter 20 0 1))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "fib-iter failed: {:?}", result);
    // fib(20) = 6765
    assert_eq!(result.unwrap().as_int(), Some(6765));
}

// =============================================================================
// Integer Fast Path Tests
// =============================================================================

#[test]
fn test_jit_int_add_wrapping() {
    // Verify i64::MAX + 1 wraps (full 64-bit integer arithmetic)

    // fn(x) -> x + 1
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
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

    let result = compile_and_call(&func, &[Value::int(i64::MAX)]).unwrap();
    // i64::MAX + 1 should wrap to i64::MIN
    assert_eq!(result.as_int(), Some(i64::MIN));
}

#[test]
fn test_jit_int_sub_wrapping() {
    // Verify i64::MIN - 1 wraps

    // fn(x) -> x - 1
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
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

    let result = compile_and_call(&func, &[Value::int(i64::MIN)]).unwrap();
    // i64::MIN - 1 should wrap to i64::MAX
    assert_eq!(result.as_int(), Some(i64::MAX));
}

#[test]
fn test_jit_div_by_zero_integer() {
    // Division by zero: fast path detects zero divisor, falls to slow path
    // fn(x, y) -> x / y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(10), Value::int(0)]).unwrap();
    // Runtime helper returns NIL on division by zero
    assert!(result.is_nil());
}

#[test]
fn test_jit_mixed_int_float_add() {
    // Mixed int + float: fast path fails (not both int), slow path handles it
    // fn(x, y) -> x + y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(1), Value::float(2.0)]).unwrap();
    assert!((result.as_float().unwrap() - 3.0).abs() < 0.001);
}

#[test]
fn test_jit_int_lt_negative() {
    // Verify sign extension is correct for negative numbers
    // fn(x, y) -> x < y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    // -5 < 3 should be true
    let result = compile_and_call(&func, &[Value::int(-5), Value::int(3)]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    // 3 < -5 should be false
    let result2 = compile_and_call(&func, &[Value::int(3), Value::int(-5)]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}

#[test]
fn test_jit_int_eq_negative() {
    // Verify equality with negative numbers
    // fn(x, y) -> x == y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    // -1 == -1 should be true
    let result = compile_and_call(&func, &[Value::int(-1), Value::int(-1)]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    // -1 == 1 should be false
    let result2 = compile_and_call(&func, &[Value::int(-1), Value::int(1)]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}

// =============================================================================
// Unary Fast Path Tests
// =============================================================================

#[test]
fn test_jit_neg_negative() {
    // fn(x) -> -x with negative input
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(-42)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_bit_not_zero() {
    // fn(x) -> ~x, bitwise NOT of 0 should be -1
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::UnaryOp {
            dst: Reg(1),
            op: UnaryOp::BitNot,
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result.as_int(), Some(-1));
}

#[test]
fn test_jit_not_integer_zero() {
    // fn(x) -> not(x), 0 is truthy in Elle so not(0) = false
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result, Value::FALSE);
}

#[test]
fn test_jit_not_empty_list() {
    // fn(x) -> not(x), empty list is truthy in Elle so not(()) = false
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[Value::EMPTY_LIST]).unwrap();
    assert_eq!(result, Value::FALSE);
}

// =============================================================================
// Fiber + JIT Gate Tests
// =============================================================================

#[test]
fn test_jit_accepts_yields_errors_signal() {
    // Signal::yields_errors() has may_suspend() = true.
    // The JIT gate now accepts this via side-exit — yielding functions
    // can be JIT-compiled and will side-exit to the interpreter on yield.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::yields_errors();

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
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept yields_errors signal via side-exit: {:?}",
        result
    );
}

#[test]
fn test_jit_accepts_errors_only_signal() {
    // Signal::errors() has may_suspend() = false.
    // The JIT gate should accept this — fiber/new, fiber/status, etc.
    // have this signal and are safe to call from JIT code.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::errors();

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
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept errors-only signal: {:?}",
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
        [is-even? (fn (n) (if (%eq n 0) true (is-odd? (%sub n 1)))) is-odd? (fn (n) (if (%eq n 0) false (is-even? (%sub n 1))))]
        (list (is-even? 10) (is-odd? 10) (is-even? 11) (is-odd? 11)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "even-odd failed: {:?}", result);
    // (is-even? 10) = true, (is-odd? 10) = false, (is-even? 11) = false, (is-odd? 11) = true
    let list = result.unwrap();
    let first = list.as_pair().unwrap();
    assert_eq!(first.first.as_bool(), Some(true)); // (is-even? 10)
    let rest1 = first.rest.as_pair().unwrap();
    assert_eq!(rest1.first.as_bool(), Some(false)); // (is-odd? 10)
    let rest2 = rest1.rest.as_pair().unwrap();
    assert_eq!(rest2.first.as_bool(), Some(false)); // (is-even? 11)
    let rest3 = rest2.rest.as_pair().unwrap();
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    // ping-pong: ping(n) -> pong(n-1), pong(n) -> ping(n-1)
    // Both are tail calls, so this should handle deep recursion
    let result = eval(
        r#"(letrec
        [ping (fn (n) (if (%eq n 0) "ping" (pong (%sub n 1)))) pong (fn (n) (if (%eq n 0) "pong" (ping (%sub n 1))))]
        (list (ping 0) (pong 0) (ping 1) (pong 1) (ping 100) (pong 100)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "ping-pong failed: {:?}", result);
    let list = result.unwrap();
    let vals: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = list;
        while let Some(pair) = cur.as_pair() {
            v.push(pair.first.with_string(|s| s.to_string()).unwrap());
            cur = pair.rest;
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining)
                true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe?
            (fn (col queens)
              (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n)
               (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n)
               (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens
           (fn (n)
             (solve-helper n 0 (list)))]

         (length (solve-nqueens 8)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
        [fa (fn (n) (if (%eq n 0) "a" (fb (%sub n 1)))) fb (fn (n) (if (%eq n 0) "b" (fc (%sub n 1)))) fc (fn (n) (if (%eq n 0) "c" (fa (%sub n 1))))]
        (list (fa 0) (fa 1) (fa 2) (fa 3) (fa 6) (fa 9)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "three-way failed: {:?}", result);
    let list = result.unwrap();
    let vals: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = list;
        while let Some(pair) = cur.as_pair() {
            v.push(pair.first.with_string(|s| s.to_string()).unwrap());
            cur = pair.rest;
        }
        v
    };
    // fa(0)=a, fa(1)=fb(0)=b, fa(2)=fb(1)=fc(0)=c,
    // fa(3)=fb(2)=fc(1)=fa(0)=a, fa(6)=a, fa(9)=a
    assert_eq!(vals, vec!["a", "b", "c", "a", "a", "a"]);
}

#[test]
fn test_jit_solo_fib_e2e() {
    // End-to-end test: solo-compiled fib with direct self-calls
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (defn fib (n) (if (%lt n 2) n (%add (fib (%sub n 1)) (fib (%sub n 2)))))
        (fib 20))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "fib(20) failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(6765));
}

#[test]
fn test_jit_batch_global_mutation_known_limitation() {
    // Documents a known Phase 1 limitation: after batch JIT compilation,
    // mutating a global (`assign`) does NOT update the direct SCC calls.
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
    let _signals = register_primitives(&mut vm, &mut symbols);

    // Define mutually recursive functions, call them enough to trigger JIT,
    // then mutate one global and call again. The result should not crash.
    let result = eval(
        r#"(begin
        (var helper (fn (n) (if (%eq n 0) "original" (helper (%sub n 1)))))
        ## Call enough times to trigger JIT compilation
        (helper 10)
        (helper 10)
        (helper 10)
        (helper 10)
        (helper 10)
        ## Mutate the global
        (assign helper (fn (n) "replaced"))
        ## Call again — may use old or new function depending on JIT state.
        ## The key invariant: this must not crash.
        (helper 5))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(
        result.is_ok(),
        "Global mutation after JIT should not crash: {:?}",
        result
    );
    // We accept either result — the point is no crash, no corruption
    let val = result.unwrap();
    assert!(val.is_string(), "Expected a string result, got: {:?}", val);
}

#[test]
fn test_jit_self_tail_call_with_list_rotation() {
    // Self-recursive function that tail-calls itself with (rest lst).
    // If JIT rotation frees the list's pair cells, this crashes.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [count-list (fn (lst acc)
               (if (empty? lst) acc
                 (count-list (rest lst) (%add acc 1))))]
            (count-list (range 200) 0))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "count-list failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(200));
}

#[test]
fn test_jit_letrec_mutual_recursion_simple() {
    // Minimal mutual recursion via letrec expression.
    // f calls g (non-tail), g calls f (non-tail). Both are silent.
    //
    // Depth 20: non-tail mutual recursion uses native stack frames.
    // With background JIT, the interpreter runs during the compilation
    // window, so depth must be safe for interpreted execution in debug
    // builds (each non-tail call adds a Rust stack frame).
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [f (fn (n) (if (%le n 0) 0 (%add 1 (g (%sub n 1))))) g (fn (n) (if (%le n 0) 0 (%add 1 (f (%sub n 1)))))]
            (f 20))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "mutual recursion failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(20));
}

#[test]
fn test_nqueens_eval_signals_are_silent() {
    // Verify that nqueens functions compiled via eval() get correct
    // (silent) signals, not SIG_YIELD from forward-ref defaults.
    use elle::pipeline::compile;
    use elle::symbol::SymbolTable;

    let source = r#"(letrec
     [check-safe-helper
        (fn (col remaining row-offset)
          (if (empty? remaining) true
            (let [placed-col (first remaining)]
              (if (or (%eq col placed-col)
                      (%eq row-offset (abs (%sub col placed-col))))
                false
                (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
       (fn (n col queens row)
         (if (%eq col n) (list)
           (if (safe? col queens)
             (let [new-queens (%pair col queens)]
               (append (solve-helper n (%add row 1) new-queens)
                       (try-cols-helper n (%add col 1) queens row)))
             (try-cols-helper n (%add col 1) queens row)))) solve-helper
       (fn (n row queens)
         (if (%eq row n) (list (reverse queens))
           (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
     (length (solve-nqueens 8)))"#;

    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");

    for constant in compiled.bytecode.constants.iter() {
        if let Some(closure) = constant.as_closure() {
            if let Some(lir) = closure.template.lir_function.as_ref() {
                let has_sc = lir.has_suspending_call();
                let signal = lir.signal;
                let name = lir.name.as_deref().unwrap_or("<anon>");
                assert!(
                    !signal.may_yield(),
                    "nqueens closure '{}' should not yield, got signal {:?}",
                    name, signal
                );
                assert!(
                    !has_sc,
                    "nqueens closure '{}' should not have SuspendingCall",
                    name
                );
            }
        }
    }
}

#[test]
fn test_nqueens_letrec_no_jit() {
    // Same nqueens letrec as test_jit_mutual_recursion_nqueens_small,
    // but with JIT disabled. If this passes and the JIT version crashes,
    // the bug is in JIT compilation of letrec closures with captures.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    vm.jit_enabled = false;
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining) true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n) (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n) (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
         (length (solve-nqueens 8)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "nqueens (no JIT) failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(92));
}

#[test]
fn test_jit_letrec_single_with_captures() {
    // Single self-recursive closure in a letrec (has captures from
    // the letrec environment). Tests JIT solo compilation of closures
    // with captures and list operations.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [count (fn (lst) (if (empty? lst) 0 (%add 1 (count (rest lst)))))]
            (count (list 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "letrec single failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(20));
}

#[test]
fn test_jit_letrec_two_closures_with_lists() {
    // Two mutually recursive closures in letrec with list operations.
    // f counts elements, g filters and calls f.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [count (fn (lst) (if (empty? lst) 0 (%add 1 (count (rest lst))))) count-after-skip (fn (lst n)
               (if (%le n 0) (count lst)
                 (if (empty? lst) 0
                   (count-after-skip (rest lst) (%sub n 1)))))]
            (count-after-skip (list 1 2 3 4 5 6 7 8 9 10) 3))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "letrec two closures failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(7));
}

#[test]
fn test_jit_letrec_nqueens_4queens() {
    // Nqueens with N=4 (only 2 solutions). Less JIT pressure than N=8.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining) true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n) (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n) (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
         (length (solve-nqueens 4)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "4-queens failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(2));
}

#[test]
fn test_nqueens_4queens_no_jit() {
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    vm.jit_enabled = false;
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining) true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n) (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n) (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
         (length (solve-nqueens 4)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "4-queens (no JIT) failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(2));
}

#[test]
fn test_jit_letrec_forward_ref_multiarg() {
    // Forward reference call with multiple arguments in letrec.
    // f calls g (forward ref) with 3 args.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [f (fn (n)
               (if (%le n 0) (list)
                 (append (g n 1 (list n)) (f (%sub n 1))))) g (fn (n offset acc)
               (if (%le offset n)
                 (g n (%add offset 1) (%pair offset acc))
                 (reverse acc)))]
            (length (f 5)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "forward ref multiarg failed: {:?}", result);
    // g(n,1,[n]) produces [n,1,2,...,n] = n+1 elements
    // f(5) = 6 + 5 + 4 + 3 + 2 + 0 = 20
    assert_eq!(result.unwrap().as_int(), Some(20));
}

// ── Nested self-tail-call rotation base tests ──────────────────────────
//
// Hypothesis: `jit_rotation_base` is a single field on `FiberHeap`.
// When an inner JIT function's self-tail-call loop calls
// `rotate_pools_jit()`, it sets this field.  The value persists after
// the inner function returns.  If the outer function then self-tail-
// calls with its own `rotate_pools_jit()`, it rotates relative to the
// inner's stale base mark — freeing objects allocated between the
// inner's base and the outer's iteration, including the outer's live
// heap values.
//
// Fix: save `jit_rotation_base` to `None` before every JIT call and
// restore after it returns (`call_jit` and `elle_jit_call`).
//
// Test design:
//   - Tests 1–2: nested self-tail-call patterns that exercise the
//     hypothesis.  Without the fix these SIGSEGV or corrupt values.
//   - Test 3: control — single (non-nested) self-tail-call loop.
//     Passes regardless of whether the fix is present because there
//     is no inner call to corrupt the rotation base.

#[test]
fn test_jit_nested_rotation_base_two_deep() {
    // Outer (`outer-loop`): self-tail-call loop that builds a list via
    //   pair and passes the growing list to the next iteration.
    // Inner (`inner-loop`): self-tail-call loop that traverses a list.
    //   This triggers `rotate_pools_jit()`, setting `jit_rotation_base`.
    //
    // Without save/restore, the outer's next `rotate_pools_jit()` uses
    // the inner's stale base, which was captured deep inside the call
    // stack.  Objects the outer allocated after that mark (pair cells)
    // get swept into the swap pool and freed one rotation later.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [inner-loop (fn (lst acc)
               (if (empty? lst) acc
                 (inner-loop (rest lst) (%add acc (first lst))))) outer-loop (fn (n acc-list)
               (if (%eq n 0)
                 (inner-loop acc-list 0)
                 (let [new-list (%pair n acc-list)]
                   (let [_ (inner-loop new-list 0)]
                     (outer-loop (%sub n 1) new-list)))))]
            (outer-loop 50 (list)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "two-deep rotation failed: {:?}", result);
    // Final inner-loop sums 1+2+…+50 = 1275
    assert_eq!(result.unwrap().as_int(), Some(1275));
}

#[test]
fn test_jit_nested_rotation_base_three_deep() {
    // Three levels of nested self-tail-call loops:
    //   c → b → a, each with self-tail-call + rotation.
    // c allocates pair cells; a and b just do integer arithmetic.
    // Without save/restore, a's rotation base leaks through b into c.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [a (fn (n acc)
               (if (%eq n 0) acc
                 (a (%sub n 1) (%add acc 1)))) b (fn (n acc)
               (if (%eq n 0) acc
                 (let [inner-sum (a 10 0)]
                   (b (%sub n 1) (%add acc inner-sum))))) c (fn (n result-list)
               (if (%eq n 0) result-list
                 (let [val (b 5 0)]
                   (c (%sub n 1) (%pair val result-list)))))]
            (let [result (c 20 (list))]
              (list (length result) (first result))))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "three-deep rotation failed: {:?}", result);
    let val = result.unwrap();
    // c: 20 iterations, each calling b(5,0) = 5×a(10,0) = 5×10 = 50
    // result = (20 50)
    assert_eq!(val.as_pair().map(|c| c.first.as_int()), Some(Some(20)));
    assert_eq!(
        val.as_pair()
            .and_then(|c| c.rest.as_pair().map(|c2| c2.first.as_int())),
        Some(Some(50))
    );
}

#[test]
fn test_jit_single_self_tail_rotation_control() {
    // Control: single self-tail-call loop traversing a list.
    // No nested JIT calls → no rotation base corruption possible.
    // Must pass regardless of whether the save/restore fix is present.
    use elle::pipeline::eval;
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [sum-list (fn (lst acc)
               (if (empty? lst) acc
                 (sum-list (rest lst) (%add acc (first lst)))))]
            (sum-list (range 500) 0))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "control rotation failed: {:?}", result);
    // range 500 = 0..499, sum = 499×500/2 = 124750
    assert_eq!(result.unwrap().as_int(), Some(124750));
}

