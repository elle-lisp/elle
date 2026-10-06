// audited: 2026-10-06
//! A frozen function keeps its header, its blocks in order, its terminators and spans, and its sites.
//!
//! docs/impl/lir.md

use super::*;
use crate::lir::{
    BasicBlock, CallSiteInfo, Label, SpannedInstr, SpannedTerminator, YieldPointInfo,
};
use crate::signals::Signal;
use crate::syntax::Span;

/// A function with every header field set to a value no default holds.
fn described() -> LirFunction {
    let mut f = LirFixture::new(Arity::Range(1, 3))
        .name("described")
        .signal(Signal::yields())
        .num_captures(2)
        .num_locals(9)
        .num_params(3)
        .num_local_params(1)
        .capture_params_mask(0b101)
        .vararg_kind(crate::hir::VarargKind::StrictStruct(vec!["k".into()]))
        .closure_id(ClosureId(6))
        .block(4, vec![], Terminator::Jump(Label(7)))
        .block(7, vec![], Terminator::Return(r(0)))
        .build_working();
    f.capture_locals_mask = crate::value::CaptureMask::from_words(vec![0, 1 << 3]);
    f.doc = Some("a docstring".into());
    f.origin = Some(Span::new(1, 2, 3, 4).with_file("header.lisp"));
    f.region_table = vec![slot(11), slot(12)];
    f.merged_slots = vec![slot(12)];
    f.frame_release_slots = vec![3, 1];
    f.frame_release_regions = vec![slot(13)];
    f
}

#[test]
fn the_header_round_trips() {
    let f = described();
    let owned = freeze(&f).expect("freezes");
    let v = owned.view();
    assert_eq!(v.name(), Some("described"));
    assert_eq!(v.arity(), Arity::Range(1, 3));
    assert_eq!(v.signal(), Signal::yields());
    assert_eq!(v.num_captures(), 2);
    assert_eq!(v.num_locals(), 9);
    assert_eq!(v.num_params(), 3);
    assert_eq!(v.num_local_params(), 1);
    assert_eq!(v.num_regs(), f.num_regs);
    assert_eq!(v.capture_params_mask(), 0b101);
    assert!(v.capture_locals_mask().is_set(67));
    assert!(!v.capture_locals_mask().is_set(66));
    assert_eq!(
        v.vararg_kind(),
        crate::hir::VarargKind::StrictStruct(vec!["k".into()])
    );
    assert_eq!(v.closure_id(), Some(ClosureId(6)));
    assert_eq!(v.entry(), Label(4));
    assert_eq!(v.doc(), Some("a docstring"));
    assert_eq!(v.origin(), f.origin);
    assert_eq!(v.region_table(), &[slot(11), slot(12)]);
    assert_eq!(v.merged_slots(), &[slot(12)]);
    assert_eq!(
        v.frame_release_slots(),
        &[1, 3],
        "ascending, so the payload's copy and this one agree on order"
    );
    assert_eq!(v.frame_release_regions(), &[slot(13)]);
}

/// Blocks keep the order the lowerer appended them in — the emitter's operand
/// depth depends on meeting every predecessor of a merge first — and each
/// keeps its label and terminator.
#[test]
fn blocks_keep_their_order_labels_and_terminators() {
    let terms = [
        Terminator::Branch {
            cond: r(1),
            then_label: Label(3),
            else_label: Label(2),
        },
        Terminator::Emit {
            signal: SignalBits::new(0xf000_0000_0000_0001),
            value: r(2),
            resume_label: Label(2),
        },
        Terminator::Jump(Label(0)),
        Terminator::Return(r(4)),
        Terminator::Unreachable,
    ];
    let labels = [5u32, 3, 2, 0, 9];
    let mut fixture = LirFixture::new(Arity::Exact(0));
    for (label, term) in labels.iter().zip(terms.iter()) {
        fixture = fixture.block(*label, vec![], *term);
    }
    let owned = freeze(&fixture.build_working()).expect("freezes");
    let view = owned.view();
    let got: Vec<(Label, String)> = view
        .blocks()
        .map(|b| (b.label(), format!("{:?}", b.terminator())))
        .collect();
    let want: Vec<(Label, String)> = labels
        .iter()
        .zip(terms.iter())
        .map(|(l, t)| (Label(*l), format!("{t:?}")))
        .collect();
    assert_eq!(got, want);
}

/// Spans survive with their files. The node names its file by an index into
/// the function's own table, so two files in one function stay apart.
#[test]
fn spans_keep_their_files() {
    let a = Span::new(10, 20, 3, 4).with_file("one.lisp");
    let b = Span::new(30, 40, 5, 6).with_file("two.lisp");
    let t = Span::new(50, 60, 7, 8);
    let mut block = BasicBlock::new(Label(0));
    block.instructions = vec![
        SpannedInstr::new(LirInstr::LoadSelf { dst: r(0) }, a),
        SpannedInstr::new(LirInstr::LoadSelf { dst: r(1) }, b),
        SpannedInstr::new(LirInstr::LoadSelf { dst: r(2) }, a),
    ];
    block.terminator = SpannedTerminator::new(Terminator::Return(r(2)), t);
    let mut f = LirFunction::new(Arity::Exact(0));
    f.blocks.push(block);
    f.num_regs = 3;
    let owned = freeze(&f).expect("freezes");
    let view = owned.view();
    let block = view.blocks().next().unwrap();
    let spans: Vec<Span> = block.nodes().map(|n| n.span()).collect();
    assert_eq!(spans, vec![a, b, a]);
    assert_eq!(block.terminator_span(), t);
    assert_eq!(owned.code().files.len(), 2, "each file once");
}

/// The sites emission finds arrive after the function froze, and read back as
/// they were recorded.
#[test]
fn sites_set_after_freezing_read_back() {
    let mut owned = frozen(vec![]);
    owned.set_sites(
        &[YieldPointInfo {
            resume_ip: 17,
            stack_regs: vec![r(3), r(1)],
            num_locals: 4,
        }],
        &[
            CallSiteInfo {
                resume_ip: 9,
                stack_regs: vec![],
                num_locals: 4,
            },
            CallSiteInfo {
                resume_ip: 30,
                stack_regs: vec![r(5)],
                num_locals: 4,
            },
        ],
    );
    let view = owned.view();
    let yp = view.yield_point(0).expect("one yield point");
    assert_eq!(
        (yp.resume_ip, yp.num_locals, yp.stack_regs),
        (17, 4, &[r(3), r(1)][..])
    );
    let ips: Vec<usize> = view.call_sites().map(|c| c.resume_ip).collect();
    assert_eq!(ips, vec![9, 30]);
    assert_eq!(view.call_site(1).unwrap().stack_regs, &[r(5)]);
    assert!(view.yield_point(1).is_none());
}

/// The plain-data half serializes and reloads — the stdlib cache and `send`
/// both rely on it — and a file crosses by spelling, so it names the same file
/// wherever it lands.
#[test]
fn the_code_survives_serialization() {
    let instrs: Vec<LirInstr> = Op::ALL.iter().map(|&op| exemplar(op)).collect();
    let mut f = working(instrs);
    f.blocks[0].instructions[0].span = Span::new(1, 2, 3, 4).with_file("ser.lisp");
    let owned = freeze(&f).expect("freezes");
    let bytes = bincode::serialize(owned.code()).expect("serializes");
    let code: LirCode = bincode::deserialize(&bytes).expect("deserializes");
    let back = LirOwned::from_parts(code, owned.values().to_vec()).expect("the values fit");
    let a: Vec<String> = owned
        .view()
        .nodes()
        .map(|n| format!("{:?} {}", n.instr(), n.span()))
        .collect();
    let b: Vec<String> = back
        .view()
        .nodes()
        .map(|n| format!("{:?} {}", n.instr(), n.span()))
        .collect();
    assert_eq!(a, b);
    assert!(b[0].ends_with("ser.lisp:3:4"));
}

/// A rebuilt function must hold the values its instructions index; one short
/// would decode a `ValueConst` past the end of the table.
#[test]
fn a_function_rebuilt_without_its_values_is_refused() {
    let owned = frozen(vec![LirInstr::ValueConst {
        dst: r(0),
        value: Value::int(1),
    }]);
    assert!(LirOwned::from_parts(owned.code().clone(), vec![]).is_err());
}
