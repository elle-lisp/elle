// audited: 2026-10-06
//! A closure crosses the send boundary with its LIR, closure-valued constants, frame release tables and rest-list layout.
//!
//! docs/threads.md

use super::*;

/// Build a minimal closure Value with an attached LIR function, on `heap`.
/// Used by the ClosureRef round-trip test.
fn make_test_closure(
    heap: *mut crate::value::fiberheap::FiberHeap,
    name: &str,
    lir: Option<LirFunction>,
) -> Value {
    let proto = TemplateProto {
        num_locals: 1,
        num_params: 1,
        lir_function: lir.map(Rc::new),
        name: Some(name.to_string()),
        ..TemplateProto::new(Vec::new(), Arity::Exact(1), Vec::new())
    };
    let closure = Closure::new(
        crate::value::closure::test_template(unsafe { &mut *heap }, proto),
        crate::value::region_slice::RegionSlice::empty(),
        SignalBits::EMPTY,
    );
    crate::value::heap::alloc(
        unsafe { &mut *heap },
        HeapObject::Closure {
            closure,
            traits: Value::NIL,
        },
    )
}

/// Build a minimal LIR function consisting of a single block that
/// loads a closure-valued ValueConst and returns it.
fn make_lir_with_closure_value_const(closure_val: Value) -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .num_params(1)
        .num_locals(1)
        .block(
            0,
            vec![LirInstr::ValueConst {
                dst: Reg(0),
                value: closure_val,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// Directly verifies the ClosureRef serialization path: a closure
/// whose LIR contains a ValueConst referencing another closure must
/// round-trip through SendBundle with its LIR preserved, and the
/// ClosureRef placeholder must be patched back to a valid ValueConst.
#[test]
fn test_send_bundle_patches_closure_value_const_in_lir() {
    crate::value::arena::with_test_region(|| {
        // One heap for the whole round-trip: the inner/outer closures and the
        // serialization all name it explicitly.
        let heap_ptr = crate::value::arena::leaked_test_heap();
        // 1. Build an inner closure (the "target" of the ValueConst).
        let inner = make_test_closure(heap_ptr, "inner", None);

        // 2. Build an outer closure whose LIR contains a ValueConst
        //    referencing `inner`. Store `inner` in the outer closure's
        //    env so it's reachable via the SendBundle intern table.
        let lir = make_lir_with_closure_value_const(inner);
        let outer_template = TemplateProto {
            num_captures: 1,
            lir_function: Some(Rc::new(lir)),
            name: Some("outer".to_string()),
            ..TemplateProto::new(Vec::new(), Arity::Exact(0), Vec::new())
        };
        // Build the env slice and the closure header in ONE explicit region
        // (slice + header must share a region), on the same heap as `inner`.
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let env = crate::value::arena::alloc_region_slice_in_region::<Value>(
            unsafe { &mut *heap_ptr },
            &[inner],
            region,
        );
        let outer_closure = Closure::new(
            crate::value::closure::test_template(unsafe { &mut *heap_ptr }, outer_template),
            // make `inner` reachable from the bundle
            env,
            SignalBits::EMPTY,
        );
        let outer_val = crate::value::arena::alloc_in_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Closure {
                closure: outer_closure,
                traits: Value::NIL,
            },
            region,
        );

        // 3. Round-trip through SendBundle.
        let bundle = SendBundle::from_value(outer_val, unsafe { &*heap_ptr }, None)
            .expect("should serialize");
        let restored = into_value_in_region(|ctx| bundle.into_value(ctx, None));

        // 4. The reconstructed outer closure should still have an LIR.
        let restored_rc = restored
            .as_closure()
            .expect("restored value should be a closure");
        let restored_lir = restored_rc
            .template
            .lir_function()
            .expect("LIR must be preserved across SendBundle round-trip");

        // 5. The LIR should contain a ValueConst (not a ClosureRef) whose
        //    value is a closure — specifically the reconstructed `inner`.
        let mut found_closure_vc = false;
        for block in &restored_lir.blocks {
            for si in &block.instructions {
                match &si.instr {
                    LirInstr::Const {
                        value: LirConst::ClosureRef(_),
                        ..
                    } => {
                        panic!("ClosureRef should have been patched during reconstruction");
                    }
                    LirInstr::ValueConst { value, .. } => {
                        assert!(
                            value.as_closure().is_some(),
                            "patched ValueConst should hold a closure"
                        );
                        found_closure_vc = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(
            found_closure_vc,
            "restored LIR must contain the patched closure ValueConst"
        );
    });
}

// ── an abandoned frame's release tables cross the boundary ───────────

#[test]
fn closure_round_trips_preserving_frame_release_tables() {
    // `frame_release_slots`/`frame_release_regions` are the table an error
    // exit walks to run the releases the abandoned frame still owed
    // (docs/impl/region/mechanism.md). A closure keeps its body across the
    // boundary, so it keeps that obligation: reconstruct it with empty tables and every
    // region an erroring worker frame owed is stranded.
    crate::value::arena::with_test_region(|| {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let child = Rc::new(TemplateProto {
            frame_release_slots: vec![21u16],
            frame_release_regions: vec![23u32],
            ..TemplateProto::new(Vec::new(), Arity::Exact(0), Vec::new())
        });
        let template = TemplateProto {
            num_locals: 1,
            num_params: 1,
            frame_release_slots: vec![3u16, 7],
            frame_release_regions: vec![11u32, 13],
            child_protos: vec![child],
            ..TemplateProto::new(Vec::new(), Arity::Exact(1), Vec::new())
        };
        let val = crate::value::heap::alloc(
            unsafe { &mut *heap_ptr },
            HeapObject::Closure {
                closure: Closure::new(
                    crate::value::closure::test_template(unsafe { &mut *heap_ptr }, template),
                    crate::value::region_slice::RegionSlice::empty(),
                    SignalBits::EMPTY,
                ),
                traits: Value::NIL,
            },
        );

        let bundle = SendBundle::from_value(val, unsafe { &*heap_ptr }, None)
            .expect("a plain closure is sendable");
        let restored = into_value_in_region(|ctx| bundle.into_value(ctx, None));

        let closure = restored.as_closure().expect("restored value is a closure");
        assert_eq!(
            closure.template.frame_release_slots(),
            &[3u16, 7],
            "the value-routed release slots must cross the boundary — an empty \
             table silently strands every region an erroring worker frame owed"
        );
        assert_eq!(
            closure.template.frame_release_regions(),
            &[11u32, 13],
            "the slot-routed release regions must cross with them; the two \
             halves of one table are useless apart"
        );
        let child = &closure.template.child_protos()[0];
        assert_eq!(
            &child.frame_release_slots[..],
            &[21u16],
            "a nested-lambda blueprint crosses via sendable_from_template and \
             template_from_sendable, not send_closure — its tables must cross too"
        );
        assert_eq!(&child.frame_release_regions[..], &[23u32]);
    });
}

// ── the rest-list layout crosses the boundary ────────────────────────

#[test]
fn closure_round_trips_preserving_its_rest_list_layout() {
    // The layout is the gate's verdict on the closure's own body
    // (docs/impl/region/restlist.md). A worker that reconstructs it at the
    // default runs correctly and claims a page per rest argument, so only the
    // field itself shows the loss. The child blueprint crosses by the template
    // path the stdlib cache also takes.
    use crate::value::RestListLayout;
    crate::value::arena::with_test_region(|| {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let child = Rc::new(TemplateProto {
            num_params: 1,
            rest_list_layout: RestListLayout::OneRegion,
            ..TemplateProto::new(Vec::new(), Arity::AtLeast(0), Vec::new())
        });
        let template = TemplateProto {
            num_locals: 1,
            num_params: 1,
            rest_list_layout: RestListLayout::OneRegion,
            child_protos: vec![child],
            ..TemplateProto::new(Vec::new(), Arity::AtLeast(0), Vec::new())
        };
        let val = crate::value::heap::alloc(
            unsafe { &mut *heap_ptr },
            HeapObject::Closure {
                closure: Closure::new(
                    crate::value::closure::test_template(unsafe { &mut *heap_ptr }, template),
                    crate::value::region_slice::RegionSlice::empty(),
                    SignalBits::EMPTY,
                ),
                traits: Value::NIL,
            },
        );

        let bundle = SendBundle::from_value(val, unsafe { &*heap_ptr }, None)
            .expect("a plain closure is sendable");
        let restored = into_value_in_region(|ctx| bundle.into_value(ctx, None));

        let closure = restored.as_closure().expect("restored value is a closure");
        assert_eq!(
            closure.template.rest_list_layout(),
            RestListLayout::OneRegion,
            "the closure's own layout crosses by send_closure"
        );
        assert_eq!(
            closure.template.child_protos()[0].rest_list_layout,
            RestListLayout::OneRegion,
            "a nested-lambda blueprint's layout crosses by the template path"
        );
    });
}
