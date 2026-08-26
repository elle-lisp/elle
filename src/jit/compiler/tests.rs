use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{BinOp, LirInstr, Reg, Terminator};
use crate::signals::Signal;
use crate::value::Arity;

fn make_simple_lir() -> LirFunction {
    // Create a simple function that returns its first argument
    // fn(x) -> x
    // The LIR uses LoadCapture to access parameters.
    // With num_captures=0, LoadCapture index 0 loads from args[0].
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

fn make_add_lir() -> LirFunction {
    // Create a function that adds two arguments
    // fn(x, y) -> x + y
    // With num_captures=0, LoadCapture index 0 and 1 load from args[0] and args[1].
    LirFixture::new(Arity::Exact(2))
        .signal(Signal::silent())
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::LoadCapture {
                    dst: Reg(1),
                    index: 1,
                },
                LirInstr::BinOp {
                    dst: Reg(2),
                    op: BinOp::Add,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
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
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::AdoptIntoActivation { child: Reg(0) },
                LirInstr::Const {
                    dst: Reg(1),
                    value: crate::lir::LirConst::Nil,
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .build()
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
                LirInstr::Compare {
                    dst: Reg(2),
                    op: CmpOp::Le,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
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
                LirInstr::BinOp {
                    dst: Reg(4),
                    op: BinOp::Sub,
                    lhs: Reg(0),
                    rhs: Reg(3),
                },
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

    let func = LirFixture::new(Arity::Exact(0))
        .signal(Signal::yields())
        .yield_points(vec![YieldPointInfo {
            resume_ip: 5,
            stack_regs: vec![],
            num_locals: 0,
        }])
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: crate::lir::LirConst::Int(42),
            }],
            Terminator::Emit {
                signal: crate::value::fiber::SIG_YIELD,
                value: Reg(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            vec![LirInstr::LoadResumeValue { dst: Reg(1) }],
            Terminator::Return(Reg(1)),
        )
        .build();

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

#[test]
fn compile_records_entry_in_code_address_registry() {
    let mut lir = make_simple_lir();
    lir.name = Some("registry-probe-solo".to_string());
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, HashMap::new(), Vec::new())
        .expect("Failed to compile");
    let entry = code.fn_ptr() as usize;
    let name = crate::jit::registry::snapshot()
        .into_iter()
        .find(|(addr, _)| *addr == entry)
        .map(|(_, name)| name);
    assert_eq!(name.as_deref(), Some("registry-probe-solo"));
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
        .compile_batch(&members, HashMap::new())
        .expect("Failed to compile batch");
    let entry = results[0].1.fn_ptr() as usize;
    let name = crate::jit::registry::snapshot()
        .into_iter()
        .find(|(addr, _)| *addr == entry)
        .map(|(_, name)| name);
    assert_eq!(name.as_deref(), Some("registry-probe-batch"));
}

/// fn() -> capture 0. With `num_captures = 1`, `LoadCapture` index 0 reads
/// through the closure environment pointer rather than an argument variable.
fn make_capture_read_lir() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .signal(Signal::silent())
        .num_captures(1)
        .block(
            0,
            vec![LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// The `load` lines of a rendered Cranelift function, in emission order.
fn load_lines(clif: &[String]) -> Vec<&str> {
    clif.iter()
        .map(|line| line.trim())
        .filter(|line| line.contains("= load."))
        .collect()
}

#[test]
fn an_argument_load_carries_trusted_flags() {
    // Trap: memory flags change the access the backend emits, never the value
    // it computes, so nothing but the rendered CLIF shows which flags a load
    // actually got.
    //
    // Counter-factual: passing `MemFlagsData::new()` instead of `::trusted()`
    // compiles, and the compiled code returns the right answers, because the
    // argument array really is aligned and mapped. It costs a trapping,
    // unaligned-tolerant access on every parameter of every hot function.
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let clif = compiler
        .clif_text(&make_simple_lir(), None)
        .expect("Failed to translate");
    let loads = load_lines(&clif);
    assert!(
        !loads.is_empty(),
        "a one-parameter function loads its argument; got:\n{}",
        clif.join("\n")
    );
    for load in &loads {
        assert!(
            load.contains("notrap aligned"),
            "argument load without trusted flags: {load}"
        );
    }
}

#[test]
fn a_capture_load_carries_trusted_flags() {
    // The environment pointer is a second base pointer, reached from a
    // different translator path than the argument array.
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let clif = compiler
        .clif_text(&make_capture_read_lir(), None)
        .expect("Failed to translate");
    let loads = load_lines(&clif);
    assert!(
        !loads.is_empty(),
        "reading capture 0 loads from the env pointer; got:\n{}",
        clif.join("\n")
    );
    for load in &loads {
        assert!(
            load.contains("notrap aligned"),
            "capture load without trusted flags: {load}"
        );
    }
}
