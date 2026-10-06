// audited: 2026-10-06
//! Every opcode survives freezing and decoding field by field, with the registers it reads and writes.
//!
//! docs/impl/lir.md

use super::*;
use crate::lir::{for_each_def, for_each_use};

/// The counter-factual is a decoder that reads one field from another's slot —
/// an `args` run taken from the wrong pool offset, a flag from the wrong bit.
/// Every field of every exemplar is distinct, so such a decoder reads back an
/// instruction that prints differently from the one frozen.
#[test]
fn every_opcode_round_trips_through_freeze_and_the_view() {
    for &op in Op::ALL {
        let instr = exemplar(op);
        assert_eq!(Op::of(&instr), op, "the exemplar of {op:?} is that opcode");
        let owned = frozen(vec![instr.clone()]);
        let view = owned.view();
        let block = view.blocks().next().expect("one block");
        let node = block.nodes().next().expect("one instruction");
        assert_eq!(node.op(), op);
        assert_eq!(
            format!("{:?}", thaw(node.instr())),
            format!("{:?}", instr),
            "{op:?} decodes to the instruction it froze from"
        );
    }
}

/// A node's uses and def are what the working form's walkers report, in the
/// same order — the WASM allocator's liveness reads them, and an operand it
/// misses dies while still live.
#[test]
fn every_opcode_reports_the_registers_the_walkers_report() {
    for &op in Op::ALL {
        let instr = exemplar(op);
        let owned = frozen(vec![instr.clone()]);
        let view = owned.view();
        let node = view.nodes().next().expect("one instruction");
        let mut uses = Vec::new();
        for_each_use(&instr, |r| uses.push(r));
        assert_eq!(node.uses(), &uses[..], "{op:?} uses");
        let mut def = None;
        for_each_def(&instr, |r| def = Some(r));
        assert_eq!(node.def(), def, "{op:?} def");
        assert_eq!(node.region(), instr.region(), "{op:?} region");
    }
}

/// Every immediate kind decodes as itself, a float bit for bit.
#[test]
fn every_immediate_constant_round_trips() {
    let consts = [
        LirConst::Nil,
        LirConst::EmptyList,
        LirConst::Bool(true),
        LirConst::Bool(false),
        LirConst::Int(i64::MIN),
        LirConst::Float(-0.0),
        LirConst::Float(f64::NAN),
        LirConst::Symbol(SymbolId::of("a-symbol")),
        LirConst::Keyword(u64::MAX),
    ];
    let instrs: Vec<LirInstr> = consts
        .iter()
        .enumerate()
        .map(|(i, c)| LirInstr::Const {
            dst: r(i as u32),
            value: c.clone(),
        })
        .collect();
    let owned = frozen(instrs.clone());
    let view = owned.view();
    let back: Vec<String> = view
        .nodes()
        .map(|n| format!("{:?}", thaw(n.instr())))
        .collect();
    let want: Vec<String> = instrs.iter().map(|i| format!("{i:?}")).collect();
    assert_eq!(back, want);
}

/// A string literal is a `MaterializeConst`, and the bytecode pool has nowhere
/// reclaimable for a string to live, so freezing refuses the one constant with
/// no frozen form and says which one it was.
#[test]
fn a_string_constant_is_refused_by_name() {
    let func = working(vec![LirInstr::Const {
        dst: r(0),
        value: LirConst::String("no".into()),
    }]);
    let err = freeze(&func).expect_err("a string constant has no frozen form");
    assert!(
        err.contains("LirConst::String"),
        "the refusal names the constant: {err}"
    );
}

/// A tail call's two release fields are the ones a decoder is likeliest to
/// drop: they ride in the pool behind the arguments, and an empty list and an
/// absent slot are the common case.
#[test]
fn a_tail_call_keeps_its_release_fields() {
    let with = exemplar(Op::TailCall);
    let without = LirInstr::TailCall {
        dst: r(9),
        func: r(8),
        args: vec![],
        arity_checked: false,
        region: slot(40),
        defer_callee_release: false,
        deferred_release_slot: None,
        borrowed_arg_slots: vec![],
    };
    let owned = frozen(vec![with.clone(), without.clone()]);
    let view = owned.view();
    let back: Vec<String> = view
        .nodes()
        .map(|n| format!("{:?}", thaw(n.instr())))
        .collect();
    assert_eq!(back, vec![format!("{with:?}"), format!("{without:?}")]);
    let first = view.nodes().next().unwrap().instr();
    match first {
        InstrRef::TailCall {
            deferred_release_slot,
            borrowed_arg_slots,
            ..
        } => {
            assert_eq!(deferred_release_slot, Some(slot(31)));
            assert_eq!(borrowed_arg_slots.iter().collect::<Vec<_>>(), vec![7, 9]);
        }
        other => panic!("decoded {other:?}"),
    }
}

/// A function's values table holds what its `ValueConst`s load, each value
/// once, and the instruction reads its own back.
#[test]
fn a_value_constant_lands_in_the_values_table() {
    let instrs = vec![
        LirInstr::ValueConst {
            dst: r(0),
            value: Value::int(5),
        },
        LirInstr::ValueConst {
            dst: r(1),
            value: Value::int(6),
        },
        LirInstr::ValueConst {
            dst: r(2),
            value: Value::int(5),
        },
    ];
    let owned = frozen(instrs);
    let view = owned.view();
    assert_eq!(view.values(), &[Value::int(5), Value::int(6)]);
    let loaded: Vec<Value> = view
        .nodes()
        .map(|n| match n.instr() {
            InstrRef::ValueConst { value, .. } => value,
            other => panic!("decoded {other:?}"),
        })
        .collect();
    assert_eq!(loaded, vec![Value::int(5), Value::int(6), Value::int(5)]);
}
