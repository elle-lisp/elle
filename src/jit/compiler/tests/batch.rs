// audited: 2026-09-06
// docs/impl/jit.md
//! What batch compilation of an SCC accepts and rejects, and that a member
//! reaches its peers through the shared module.

use super::*;

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
        .compile_batch(&members)
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
    let f = LirFixture::new(Arity::Exact(1))
        .name("f")
        .signal(Signal::silent())
        // Block 0 (entry): load arg, check condition
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::Int(0),
                },
                LirInstr::compare(Reg(2), CmpOp::Le, Reg(0), Reg(1)),
            ],
            Terminator::Branch {
                cond: Reg(2),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        // Block 1 (base case): return x
        .block(1, vec![], Terminator::Return(Reg(0)))
        // Block 2 (recursive case): call g(x - 1)
        .block(
            2,
            vec![
                LirInstr::Const {
                    dst: Reg(3),
                    value: LirConst::Int(1),
                },
                LirInstr::binop(Reg(4), BinOp::Sub, Reg(0), Reg(3)),
                LirInstr::ValueConst {
                    dst: Reg(5),
                    value: crate::value::Value::NIL,
                },
                LirInstr::Call {
                    dst: Reg(6),
                    func: Reg(5),
                    args: vec![Reg(4)],
                    arity_checked: false,
                    region: crate::hir::region::StaticRegion::new(2).unwrap(),
                },
            ],
            Terminator::Return(Reg(6)),
        )
        .build();

    // Build g: tail-call f(x)
    let g = LirFixture::new(Arity::Exact(1))
        .name("g")
        .signal(Signal::silent())
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::ValueConst {
                    dst: Reg(1),
                    value: crate::value::Value::NIL,
                },
                LirInstr::TailCall {
                    dst: Reg(2),
                    func: Reg(1),
                    args: vec![Reg(0)],
                    arity_checked: false,
                    region: crate::hir::region::StaticRegion::new(2).unwrap(),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: Vec::new(),
                },
            ],
            Terminator::Unreachable,
        )
        .build();

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
        .compile_batch(&members)
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
    let result = compiler.compile_batch(&members);
    assert!(matches!(result, Err(JitError::Polymorphic)));
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
    let result = compiler.compile_batch(&members);
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
    let result = compiler.compile_batch(&members);
    assert!(
        result.is_ok(),
        "List variadic functions should compile in batch: {:?}",
        result.err(),
    );
}

#[test]
fn compile_batch_records_each_member_in_code_address_registry() {
    let mut lir = make_simple_lir();
    lir.name = Some("registry-probe-batch".to_string());
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let members = vec![BatchMember {
        sym: SymbolId(7),
        lir: &lir,
    }];
    let results = compiler
        .compile_batch(&members)
        .expect("Failed to compile batch");
    let entry = results[0].1.fn_ptr() as usize;
    let name = crate::jit::registry::snapshot()
        .into_iter()
        .find(|(addr, _)| *addr == entry)
        .map(|(_, name)| name);
    assert_eq!(name.as_deref(), Some("registry-probe-batch"));
}
