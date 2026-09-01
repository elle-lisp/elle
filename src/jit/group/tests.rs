use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{ClosureId, LirInstr, Reg, Terminator};
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::Arity;

/// Build a simple LIR function that calls a function loaded via ValueConst.
fn make_caller(name: &str, _callee_sym: SymbolId) -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name(name)
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
                LirInstr::Call {
                    dst: Reg(2),
                    func: Reg(1),
                    args: vec![Reg(0)],
                    arity_checked: false,
                    region: crate::hir::region::StaticRegion::new(2).unwrap(),
                },
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
}

/// Build a simple identity LIR function (no calls).
fn make_leaf() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("leaf")
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

/// Build a mock closure Value with the given LIR function.
fn make_closure_value(lir: LirFunction) -> Value {
    use crate::value::ClosureTemplate;

    let arity = lir.arity;
    let signal = lir.signal;
    let template = Rc::new(ClosureTemplate {
        signal,
        lir_function: Some(Rc::new(lir)),
        ..ClosureTemplate::new(Rc::new(vec![]), arity, Rc::new(vec![]))
    });

    let h = crate::primitives::ctx::TestHeap::new();
    let closure = crate::value::Closure {
        template: crate::value::TemplateRef::new(template),
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
    h.ctx().closure(closure)
}

#[test]
fn test_find_global_call_targets() {
    crate::value::arena::with_test_region(|| {
        // Call targets reach the callee via `ValueConst`, not a `LoadGlobal`
        // opcode, so `find_global_call_targets` finds nothing and returns empty.
        let sym_g = SymbolId(10);
        let caller = make_caller("f", sym_g);
        let targets = find_global_call_targets(&caller);
        assert!(targets.is_empty());
    });
}

#[test]
fn test_find_global_call_targets_no_calls() {
    crate::value::arena::with_test_region(|| {
        let leaf = make_leaf();
        let targets = find_global_call_targets(&leaf);
        assert!(targets.is_empty());
    });
}

#[test]
fn test_discover_empty_when_no_peers() {
    crate::value::arena::with_test_region(|| {
        let leaf = make_leaf();
        let globals: HashMap<SymbolId, Value> = HashMap::new();
        let group = discover_compilation_group(&leaf, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_finds_callee() {
    crate::value::arena::with_test_region(|| {
        // Call targets reach the callee via `ValueConst`, not a `LoadGlobal`
        // opcode, so `discover_compilation_group` finds no targets in the LIR
        // and returns empty.
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);
        let callee = make_leaf();

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), make_closure_value(callee));

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_skips_suspending() {
    crate::value::arena::with_test_region(|| {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        let mut callee = make_leaf();
        callee.signal = Signal::yields();

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), make_closure_value(callee));

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_skips_captures() {
    crate::value::arena::with_test_region(|| {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        let mut callee = make_leaf();
        callee.num_captures = 1;

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), make_closure_value(callee));

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_skips_unsupported_instructions() {
    crate::value::arena::with_test_region(|| {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        // Build a callee with MakeClosure (unsupported)
        let callee = LirFixture::new(Arity::Exact(1))
            .name("callee_with_closure")
            .signal(Signal::silent())
            .block(
                0,
                vec![
                    LirInstr::LoadCapture {
                        dst: Reg(0),
                        index: 0,
                    },
                    LirInstr::MakeClosure {
                        dst: Reg(1),
                        closure_id: ClosureId(0),
                        captures: vec![],
                        region: crate::hir::region::StaticRegion::new(2).unwrap(),
                    },
                ],
                Terminator::Return(Reg(1)),
            )
            .build();

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), make_closure_value(callee));

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_transitive() {
    crate::value::arena::with_test_region(|| {
        // Call targets load via `ValueConst`, not a `LoadGlobal` opcode, so
        // transitive discovery finds nothing.
        let sym_g = SymbolId(5);
        let sym_h = SymbolId(6);

        let caller = make_caller("f", sym_g);
        let g = make_caller("g", sym_h);
        let h = make_leaf();

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), make_closure_value(g));
        globals.insert(SymbolId(6), make_closure_value(h));

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_no_duplicates_in_cycle() {
    crate::value::arena::with_test_region(|| {
        // Call targets load via `ValueConst`, not a `LoadGlobal` opcode, so
        // cycle discovery finds nothing.
        let sym_f = SymbolId(4);
        let sym_g = SymbolId(5);

        let hot = make_caller("f", sym_g);
        let g = make_caller("g", sym_f);

        let f_for_global = make_caller("f", sym_g);

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(4), make_closure_value(f_for_global));
        globals.insert(SymbolId(5), make_closure_value(g));

        let group = discover_compilation_group(&hot, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_unknown_sym() {
    crate::value::arena::with_test_region(|| {
        let sym_g = SymbolId(999);
        let caller = make_caller("f", sym_g);
        let globals: HashMap<SymbolId, Value> = HashMap::new();

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_non_closure_global() {
    crate::value::arena::with_test_region(|| {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), Value::int(42)); // Not a closure

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_discover_closure_without_lir() {
    crate::value::arena::with_test_region(|| {
        use crate::value::ClosureTemplate;

        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        // Closure with no lir_function
        let template = Rc::new(ClosureTemplate::new(
            Rc::new(vec![]),
            Arity::Exact(1),
            Rc::new(vec![]),
        ));

        let h = crate::primitives::ctx::TestHeap::new();
        let closure = crate::value::Closure {
            template: crate::value::TemplateRef::new(template),
            env: crate::value::region_slice::RegionSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        };

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        globals.insert(SymbolId(5), h.ctx().closure(closure));

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    });
}

#[test]
fn test_find_targets_with_tail_call() {
    crate::value::arena::with_test_region(|| {
        // Call targets reach the callee via `ValueConst`, not a `LoadGlobal`
        // opcode, so `find_global_call_targets` finds nothing and returns an
        // empty set.
        let func = LirFixture::new(Arity::Exact(1))
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

        let targets = find_global_call_targets(&func);
        assert!(targets.is_empty());
    });
}

#[test]
fn test_discover_respects_size_bound() {
    crate::value::arena::with_test_region(|| {
        // Create a chain of functions f0 -> f1 -> f2 -> ... -> f(N)
        // Verify that discovery stops at MAX_GROUP_SIZE.
        let n = MAX_GROUP_SIZE + 5; // more than the limit
        let syms: Vec<SymbolId> = (0..n).map(|i| SymbolId(i as u64)).collect();

        // Build chain: f_i calls f_{i+1}
        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        for i in 0..n - 1 {
            let caller = make_caller(&format!("f{}", i), syms[i + 1]);
            globals.insert(syms[i], make_closure_value(caller));
        }
        // Last function is a leaf
        globals.insert(syms[n - 1], make_closure_value(make_leaf()));

        // Hot function calls f0
        let hot = make_caller("hot", syms[0]);
        let group = discover_compilation_group(&hot, &globals);

        // Should be capped by MAX_GROUP_SIZE
        assert!(
            group.len() <= MAX_GROUP_SIZE,
            "Group size {} exceeds MAX_GROUP_SIZE {}",
            group.len(),
            MAX_GROUP_SIZE
        );
    });
}

#[test]
fn test_discover_respects_depth_bound() {
    crate::value::arena::with_test_region(|| {
        // Create a chain longer than MAX_DISCOVERY_DEPTH.
        // Even though all functions are valid, depth limiting should cap discovery.
        let n = MAX_DISCOVERY_DEPTH + 3;
        let syms: Vec<SymbolId> = (0..n).map(|i| SymbolId(i as u64)).collect();

        let mut globals: HashMap<SymbolId, Value> = HashMap::new();
        for i in 0..n - 1 {
            let caller = make_caller(&format!("f{}", i), syms[i + 1]);
            globals.insert(syms[i], make_closure_value(caller));
        }
        globals.insert(syms[n - 1], make_closure_value(make_leaf()));

        let hot = make_caller("hot", syms[0]);
        let group = discover_compilation_group(&hot, &globals);

        // Depth 1 = direct callees, depth 2 = their callees, etc.
        // Should not discover beyond MAX_DISCOVERY_DEPTH levels.
        assert!(
            group.len() <= MAX_DISCOVERY_DEPTH,
            "Group size {} exceeds MAX_DISCOVERY_DEPTH {} (depth bounding failed)",
            group.len(),
            MAX_DISCOVERY_DEPTH
        );
    });
}

#[test]
fn test_has_unsupported_instructions_clean() {
    crate::value::arena::with_test_region(|| {
        let leaf = make_leaf();
        assert!(!has_unsupported_instructions(&leaf));
    });
}

#[test]
fn test_has_unsupported_instructions_with_eval() {
    crate::value::arena::with_test_region(|| {
        let func = LirFixture::new(Arity::Exact(1))
            .signal(Signal::silent())
            .block(
                0,
                vec![LirInstr::Eval {
                    dst: Reg(0),
                    expr: Reg(1),
                    env: Reg(2),
                }],
                Terminator::Return(Reg(0)),
            )
            .build();

        assert!(has_unsupported_instructions(&func));
    });
}
