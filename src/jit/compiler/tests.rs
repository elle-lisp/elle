use super::*;
use crate::lir::{BasicBlock, BinOp, LirInstr, Reg, SpannedInstr, SpannedTerminator, Terminator};
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::Arity;

fn make_simple_lir() -> LirFunction {
    // Create a simple function that returns its first argument
    // fn(x) -> x
    // The LIR uses LoadCapture to access parameters.
    // With num_captures=0, LoadCapture index 0 loads from args[0].
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    // Load argument 0 into register 0
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCapture {
            dst: Reg(0),
            index: 0,
        },
        Span::synthetic(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

    func.blocks.push(entry);
    func.entry = Label(0);
    func
}

fn make_add_lir() -> LirFunction {
    // Create a function that adds two arguments
    // fn(x, y) -> x + y
    // With num_captures=0, LoadCapture index 0 and 1 load from args[0] and args[1].
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    // Load arguments into registers
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCapture {
            dst: Reg(0),
            index: 0,
        },
        Span::synthetic(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCapture {
            dst: Reg(1),
            index: 1,
        },
        Span::synthetic(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        Span::synthetic(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), Span::synthetic());

    func.blocks.push(entry);
    func.entry = Label(0);
    func
}

#[test]
fn test_compile_identity() {
    let lir = make_simple_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, HashMap::new(), Vec::new())
        .expect("Failed to compile");

    // Call the compiled function with self_tag=0, self_payload=0 (no self-tail-call).
    // A real VM is required: every compiled function's prologue pushes an
    // activation region-map frame (`elle_jit_push_region_map`).
    let mut vm = crate::vm::VM::new();
    let args = [crate::value::Value::int(42)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    assert_eq!(value.as_int(), Some(42));
}

#[test]
fn test_compile_add() {
    let lir = make_add_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, HashMap::new(), Vec::new())
        .expect("Failed to compile");

    // Call the compiled function with self_tag=0, self_payload=0
    let mut vm = crate::vm::VM::new();
    let args = [crate::value::Value::int(10), crate::value::Value::int(32)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            2,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    assert_eq!(value.as_int(), Some(42));
}

/// fn(x) -> nil, adopting x's region into the current activation's owner node.
/// The compiled body: load arg 0, `AdoptIntoActivation`, return nil.
fn make_adopt_into_activation_lir() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::LoadCapture {
            dst: Reg(0),
            index: 0,
        },
        Span::synthetic(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::AdoptIntoActivation { child: Reg(0) },
        Span::synthetic(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: crate::lir::LirConst::Nil,
        },
        Span::synthetic(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), Span::synthetic());

    func.blocks.push(entry);
    func.entry = Label(0);
    func
}

/// End-to-end exercise of the ACTIVATION OWNER NODE on the JIT
/// (docs/impl/region/owner.md § "Owner nodes — an activation as a forest root"),
/// the compiled twin of
/// `runtime::tests::ownership::activation_owner_node_frees_adopted_member_on_normal_completion`.
/// The compiled body adopts its argument's region into the activation's
/// lazily-minted owner node (`elle_jit_adopt_into_activation`); the compiled
/// `Return` path must free the node (`elle_jit_release_activation_owner_node`),
/// whose subtree drop reclaims the member — its generation bumps and the live
/// region count stays bounded across 50 calls. The member is Owned (count
/// consumed by the adopt), so if the Return-path release is not emitted, NOTHING
/// reclaims it — node + member entries survive every call.
#[test]
fn adopt_into_activation_frees_member_at_compiled_return() {
    use crate::value::heap::{HeapObject, Pair};

    let lir = make_adopt_into_activation_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, HashMap::new(), Vec::new())
        .expect("Failed to compile");

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = crate::value::arena::alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(
                crate::value::Value::NIL,
                crate::value::Value::NIL,
            )),
        );
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        let args = [child];
        let value = unsafe {
            code.call(
                std::ptr::null(),
                args.as_ptr(),
                1,
                &mut vm as *mut crate::vm::VM as *mut (),
                0,
                0,
            )
        }
        .to_value();
        assert!(value.is_nil(), "the adopt-and-return body returns nil");

        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the owner node's release on the compiled Return path \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each compiled call's completion — live \
         region count must not grow (baseline={baseline}, after 50 calls={after})",
    );
}

#[test]
fn test_accept_polymorphic() {
    let mut lir = make_simple_lir();
    lir.signal = Signal::polymorphic(0);

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept polymorphic functions (runtime dispatch handles callables): {:?}",
        result,
    );
}

#[test]
fn test_accept_yielding() {
    let mut lir = make_simple_lir();
    lir.signal = Signal::yields();

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    // Should compile (no Yield terminators in this simple LIR)
    let result = compiler.compile(&lir, None, HashMap::new(), Vec::new());
    assert!(result.is_ok());
}

#[test]
fn test_compile_batch_single_function() {
    // A batch with one function should work identically to compile()
    let lir = make_simple_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let members = vec![BatchMember {
        sym: SymbolId(0),
        lir: &lir,
    }];
    let results = compiler
        .compile_batch(&members, HashMap::new())
        .expect("Failed to compile batch");

    assert_eq!(results.len(), 1);
    let (sym, code) = &results[0];
    assert_eq!(*sym, SymbolId(0));

    let mut vm = crate::vm::VM::new();
    let args = [crate::value::Value::int(42)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    assert_eq!(value.as_int(), Some(42));
}

#[test]
fn test_compile_batch_mutual_calls() {
    // Two functions that call each other via ValueConst + Call.
    // f(x) = if x <= 0 then x else g(x - 1)
    // g(x) = f(x)  (just forwards to f)
    //
    // We can't actually CALL these without a VM (the direct SCC calls
    // still need a valid vm pointer for exception checks), but this test
    // verifies that batch compilation with cross-references succeeds.
    use crate::lir::{CmpOp, LirConst};

    let sym_f = SymbolId(100);
    let sym_g = SymbolId(101);

    // Build f: if x <= 0 then x else call g(x - 1)
    let mut f = LirFunction::new(Arity::Exact(1));
    f.name = Some("f".to_string());
    f.num_regs = 8;
    f.num_captures = 0;
    f.signal = Signal::silent();

    // Block 0 (entry): load arg, check condition
    let mut b0 = BasicBlock::new(Label(0));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::LoadCapture {
            dst: Reg(0),
            index: 0,
        },
        Span::synthetic(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        Span::synthetic(),
    ));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Le,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        Span::synthetic(),
    ));
    b0.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(2),
            then_label: Label(1),
            else_label: Label(2),
        },
        Span::synthetic(),
    );

    // Block 1 (base case): return x
    let mut b1 = BasicBlock::new(Label(1));
    b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

    // Block 2 (recursive case): call g(x - 1)
    let mut b2 = BasicBlock::new(Label(2));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(3),
            value: LirConst::Int(1),
        },
        Span::synthetic(),
    ));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(4),
            op: BinOp::Sub,
            lhs: Reg(0),
            rhs: Reg(3),
        },
        Span::synthetic(),
    ));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::ValueConst {
            dst: Reg(5),
            value: crate::value::Value::NIL,
        },
        Span::synthetic(),
    ));
    b2.instructions.push(SpannedInstr::new(
        LirInstr::Call {
            dst: Reg(6),
            func: Reg(5),
            args: vec![Reg(4)],
            arity_checked: false,
            region: crate::hir::region::StaticRegion::new(2).unwrap(),
        },
        Span::synthetic(),
    ));
    b2.terminator = SpannedTerminator::new(Terminator::Return(Reg(6)), Span::synthetic());

    f.blocks = vec![b0, b1, b2];
    f.entry = Label(0);

    // Build g: tail-call f(x)
    let mut g = LirFunction::new(Arity::Exact(1));
    g.name = Some("g".to_string());
    g.num_regs = 4;
    g.num_captures = 0;
    g.signal = Signal::silent();

    let mut gb0 = BasicBlock::new(Label(0));
    gb0.instructions.push(SpannedInstr::new(
        LirInstr::LoadCapture {
            dst: Reg(0),
            index: 0,
        },
        Span::synthetic(),
    ));
    gb0.instructions.push(SpannedInstr::new(
        LirInstr::ValueConst {
            dst: Reg(1),
            value: crate::value::Value::NIL,
        },
        Span::synthetic(),
    ));
    gb0.instructions.push(SpannedInstr::new(
        LirInstr::TailCall {
            dst: Reg(2),
            func: Reg(1),
            args: vec![Reg(0)],
            arity_checked: false,
            region: crate::hir::region::StaticRegion::new(2).unwrap(),
            adopt_callee: false,
            adopt_region_slot: None,
        },
        Span::synthetic(),
    ));
    gb0.terminator = SpannedTerminator::new(Terminator::Unreachable, Span::synthetic());

    g.blocks = vec![gb0];
    g.entry = Label(0);

    // Compile both together
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let members = vec![
        BatchMember {
            sym: sym_f,
            lir: &f,
        },
        BatchMember {
            sym: sym_g,
            lir: &g,
        },
    ];
    let results = compiler
        .compile_batch(&members, HashMap::new())
        .expect("Failed to compile batch");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, sym_f);
    assert_eq!(results[1].0, sym_g);
}

#[test]
fn test_compile_batch_rejects_polymorphic() {
    let mut lir = make_simple_lir();
    lir.signal = Signal::polymorphic(0);

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let members = vec![BatchMember {
        sym: SymbolId(0),
        lir: &lir,
    }];
    let result = compiler.compile_batch(&members, HashMap::new());
    assert!(matches!(result, Err(JitError::Polymorphic)));
}

#[test]
fn test_compile_yielding_function() {
    use crate::lir::YieldPointInfo;

    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::yields();

    let mut b0 = BasicBlock::new(Label(0));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: crate::lir::LirConst::Int(42),
        },
        Span::synthetic(),
    ));
    b0.terminator = SpannedTerminator::new(
        Terminator::Emit {
            signal: crate::value::fiber::SIG_YIELD,
            value: Reg(0),
            resume_label: Label(1),
        },
        Span::synthetic(),
    );

    let mut b1 = BasicBlock::new(Label(1));
    b1.instructions.push(SpannedInstr::new(
        LirInstr::LoadResumeValue { dst: Reg(1) },
        Span::synthetic(),
    ));
    b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), Span::synthetic());

    func.blocks = vec![b0, b1];
    func.entry = Label(0);
    func.yield_points = vec![YieldPointInfo {
        resume_ip: 5,
        stack_regs: vec![],
        num_locals: 0,
    }];

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&func, None, HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "Yielding function should compile: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().yield_points.len(), 1);
}

#[test]
fn test_reject_struct_variadic() {
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::Struct;

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, HashMap::new(), Vec::new());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "Struct variadic functions should be rejected: {:?}",
        result,
    );
}

#[test]
fn test_reject_strict_struct_variadic() {
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::StrictStruct(vec!["key".to_string()]);

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, HashMap::new(), Vec::new());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "StrictStruct variadic functions should be rejected: {:?}",
        result,
    );
}

#[test]
fn test_compile_list_variadic() {
    // AtLeast(1) + VarargKind::List should now compile successfully.
    // fn(x & rest) -> x  (ignores rest, just returns first arg)
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::List;
    lir.num_params = 2; // x + rest

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "List variadic functions should compile: {:?}",
        result.err(),
    );
}

#[test]
fn test_compile_batch_rejects_struct_variadic() {
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::Struct;

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let members = vec![BatchMember {
        sym: SymbolId(0),
        lir: &lir,
    }];
    let result = compiler.compile_batch(&members, HashMap::new());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "Struct variadic functions should be rejected from batch: {:?}",
        result,
    );
}

#[test]
fn test_compile_batch_accepts_list_variadic() {
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::List;
    lir.num_params = 2;

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let members = vec![BatchMember {
        sym: SymbolId(0),
        lir: &lir,
    }];
    let result = compiler.compile_batch(&members, HashMap::new());
    assert!(
        result.is_ok(),
        "List variadic functions should compile in batch: {:?}",
        result.err(),
    );
}
