// audited: 2026-10-06
//! Every opcode survives encoding and decoding field by field, with the registers it reads and writes.
//!
//! docs/impl/lir.md

use super::regs::{expected_def, expected_uses};
use super::*;

/// The counter-factual is a decoder that reads one field from another's slot —
/// an `args` run taken from the wrong pool offset, a flag from the wrong bit.
/// Every field of every exemplar is distinct, so such a decoder reads back an
/// instruction that differs from the one encoded.
#[test]
fn every_opcode_round_trips_through_emit_and_the_view() {
    for &op in Op::ALL {
        let instr = exemplar(op);
        assert_eq!(Op::of(&instr), op, "the exemplar of {op:?} is that opcode");
        let owned = frozen(&[instr]);
        let view = owned.view();
        let block = view.blocks().next().expect("one block");
        let node = block.nodes().next().expect("one instruction");
        assert_eq!(node.op(), op);
        assert_eq!(
            node.instr(),
            instr,
            "{op:?} decodes to the instruction it was built as"
        );
    }
}

/// A node's uses and def are the registers its instruction names, in field
/// order — the WASM allocator's liveness reads them, and an operand it misses
/// dies while still live.
#[test]
fn every_opcode_reports_the_registers_its_instruction_names() {
    for &op in Op::ALL {
        let instr = exemplar(op);
        let owned = frozen(&[instr]);
        let view = owned.view();
        let node = view.nodes().next().expect("one instruction");
        assert_eq!(node.uses(), &expected_uses(&instr)[..], "{op:?} uses");
        assert_eq!(node.def(), expected_def(&instr), "{op:?} def");
        assert_eq!(node.region(), instr.region(), "{op:?} region");
    }
}

/// Every immediate kind decodes as itself, a float bit for bit.
#[test]
fn every_immediate_constant_round_trips() {
    let consts = [
        ConstRef::Nil,
        ConstRef::EmptyList,
        ConstRef::Bool(true),
        ConstRef::Bool(false),
        ConstRef::Int(i64::MIN),
        ConstRef::Float(-0.0),
        ConstRef::Float(f64::NAN),
        ConstRef::Symbol(SymbolId::of("a-symbol")),
        ConstRef::Keyword(u64::MAX),
    ];
    let instrs: Vec<InstrRef<'_>> = consts
        .iter()
        .enumerate()
        .map(|(i, c)| InstrRef::Const {
            dst: r(i as u32),
            value: *c,
        })
        .collect();
    let owned = frozen(&instrs);
    let view = owned.view();
    // Compared as text, because a NaN is not equal to itself and its bits are
    // what must survive.
    let back: Vec<String> = view.nodes().map(|n| format!("{:?}", n.instr())).collect();
    let want: Vec<String> = instrs.iter().map(|i| format!("{i:?}")).collect();
    assert_eq!(back, want);
    let nan = view.nodes().nth(6).map(|n| n.instr());
    assert!(
        matches!(nan, Some(InstrRef::Const { value: ConstRef::Float(f), .. }) if f.to_bits() == f64::NAN.to_bits()),
        "the NaN keeps its bits: {nan:?}"
    );
}

/// A tail call's two release fields are the ones a decoder is likeliest to
/// drop: they ride in the pool behind the arguments, and an empty list and an
/// absent slot are the common case.
#[test]
fn a_tail_call_keeps_its_release_fields() {
    let with = exemplar(Op::TailCall);
    let without = InstrRef::TailCall {
        dst: r(9),
        func: r(8),
        args: &[],
        arity_checked: false,
        region: slot(40),
        defer_callee_release: false,
        deferred_release_slot: None,
        borrowed_arg_slots: Slots::new(&[]),
    };
    let owned = frozen(&[with, without]);
    let view = owned.view();
    let back: Vec<InstrRef<'_>> = view.nodes().map(|n| n.instr()).collect();
    assert_eq!(back, vec![with, without]);
    match back[0] {
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
    let owned = frozen(&[
        InstrRef::ValueConst {
            dst: r(0),
            value: Value::int(5),
        },
        InstrRef::ValueConst {
            dst: r(1),
            value: Value::int(6),
        },
        InstrRef::ValueConst {
            dst: r(2),
            value: Value::int(5),
        },
    ]);
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
