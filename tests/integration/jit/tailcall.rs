// audited: 2026-09-06
// src/jit/AGENTS.md
// Tail calls the JIT turns into a loop, and the integer fast paths at the
// edges: wrapping overflow, a zero divisor, a mixed int/float pair.

use super::*;

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
            dst: Reg(1),
            func: Reg(0),
            args: vec![],
            arity_checked: false,
            defer_callee_release: false,
            deferred_release_slot: None,
            borrowed_arg_slots: Vec::new(),
            region: elle::hir::region::StaticRegion::new(2).unwrap(),
        },
        span(),
    ));
    // TailCall emits a return, so we need Unreachable as the terminator
    entry.terminator = SpannedTerminator::new(Terminator::Unreachable, span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func, None, Vec::new());
    // TailCall should now compile successfully
    assert!(result.is_ok(), "TailCall should compile: {:?}", result);
}

// =============================================================================
// Self-Tail-Call Optimization Tests (End-to-End)
// =============================================================================

#[test]
fn test_jit_self_tail_call_loop() {
    // This should compile to a native loop, not bounce to interpreter
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
        LirInstr::binop(Reg(2), BinOp::Add, Reg(0), Reg(1)),
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
        LirInstr::binop(Reg(2), BinOp::Sub, Reg(0), Reg(1)),
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
        LirInstr::binop(Reg(2), BinOp::Div, Reg(0), Reg(1)),
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
        LirInstr::binop(Reg(2), BinOp::Add, Reg(0), Reg(1)),
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
        LirInstr::compare(Reg(2), CmpOp::Lt, Reg(0), Reg(1)),
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
        LirInstr::compare(Reg(2), CmpOp::Eq, Reg(0), Reg(1)),
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
