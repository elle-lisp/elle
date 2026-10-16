// audited: 2026-10-14
// docs/impl/region/template.md
// docs/impl/graft.md
//! What the emitter writes into a nested lambda's payload: the release tables, the rest-list layout and the fragment.

use super::*;
use crate::lir::code::LirOwned;

/// The unit of a nullary entry whose one `MakeClosure` builds `nested`, so a
/// test reads what the emitter wrote into the nested lambda's payload off
/// `unit.entry().child(0)`.
fn unit_with_child(nested: LirOwned) -> crate::value::CodeUnit {
    use crate::hir::region::StaticRegion;
    use crate::lir::ClosureId;

    let outer = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: &[],
                region: StaticRegion::new(2).unwrap(),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();
    let module = FrozenModule {
        entry: outer,
        closures: vec![nested],
    };
    let code = CodeArena::mint(unsafe { &mut *crate::value::arena::leaked_test_heap() });
    let bytecode = Emitter::new(code).emit_module(&module).bytecode;
    crate::value::CodeUnit::new(code, bytecode)
}

/// A nested lambda's payload carries both release tables, so a discard of a
/// parked frame of a closure the emitted `MakeClosure` builds runs the releases
/// that frame still owes (docs/impl/region/template.md).
#[test]
fn a_nested_lambdas_payload_carries_the_frame_release_tables() {
    // Counter-factual: a payload built with both tables empty fails nothing
    // that runs. The closure built over it carries real bytecode and returns
    // the right answers; what it loses is the walk a discard of its parked
    // frame runs, which strands every region that frame still owed.
    use crate::hir::region::StaticRegion;

    let nested = LirFixture::new(Arity::Exact(0))
        .name("nested")
        .head(|h| {
            h.frame_release_slots = vec![3, 7];
            h.frame_release_regions = vec![
                StaticRegion::new(11).unwrap(),
                StaticRegion::new(13).unwrap(),
            ];
        })
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let unit = unit_with_child(nested);
    assert_eq!(
        unit.entry().num_children(),
        1,
        "one MakeClosure registers one child"
    );
    let child = unit.entry().child(0);
    assert_eq!(
        child.frame_release_slots(),
        &[3u16, 7],
        "the value route's slots reach the payload",
    );
    assert_eq!(
        child.frame_release_regions(),
        &[11u32, 13],
        "the slot route's regions reach the payload",
    );
}

/// A nested lambda's payload carries the rest-list layout the gate wrote onto
/// its `LirHead`, so a closure the emitted `MakeClosure` builds builds its rest
/// list the way the analysis proved it may (docs/impl/region/restlist.md).
#[test]
fn a_nested_lambdas_payload_carries_its_rest_list_layout() {
    // Counter-factual: a payload left at the default layout runs correctly and
    // claims a page per rest argument, which no answer the closure returns can
    // show.
    use crate::value::RestListLayout;

    let nested = LirFixture::new(Arity::AtLeast(0))
        .name("nested")
        .num_params(1)
        .num_locals(1)
        .head(|h| h.rest_list_layout = RestListLayout::OneRegion)
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let unit = unit_with_child(nested);
    assert_eq!(
        unit.entry().child(0).rest_list_layout(),
        RestListLayout::OneRegion,
        "the gate's verdict reaches the payload"
    );
}

/// A nested lambda's payload carries the fragment bytes the lowerer put on its
/// `LirHead`, so a graft in a later unit reads the body off the closure's code
/// object (docs/impl/graft.md). The payload never decodes them, so any bytes
/// pin the path.
#[test]
fn a_nested_lambdas_payload_carries_its_fragment() {
    // Counter-factual: a payload written with the field empty runs correctly,
    // and every call to the closure from another unit stays a call.
    let nested = LirFixture::new(Arity::Exact(0))
        .name("nested")
        .head(|h| h.fragment = vec![7, 0, 7])
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let unit = unit_with_child(nested);
    assert_eq!(
        unit.entry().child(0).fragment(),
        &[7u8, 0, 7],
        "the bytes the lowerer put on the head reach the payload"
    );
}
