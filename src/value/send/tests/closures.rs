// audited: 2026-10-06
//! A closure crosses the send boundary with its LIR, closure-valued constants, frame release tables and rest-list layout.
//!
//! docs/threads.md

use super::*;

/// Build a minimal closure Value with an attached LIR function, on `heap`.
fn make_test_closure(
    heap: *mut crate::value::fiberheap::FiberHeap,
    name: &str,
    lir: Option<LirOwned>,
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

/// A closure's LIR crosses with the values its `ValueConst`s load — a closure
/// and a heap list here — and the worker's JIT compiles what arrives. The
/// counter-factual is a sender that cannot carry one of the two and drops the
/// LIR: the closure still runs on the interpreter with the right answer, so
/// only the arriving LIR and its compile can tell.
#[test]
fn a_closure_crosses_with_its_lir_and_the_values_it_loads() {
    crate::value::arena::with_test_region(|| {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let inner = make_test_closure(heap_ptr, "inner", None);
        let list = crate::value::heap::alloc(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(crate::value::heap::Pair {
                first: Value::int(1),
                rest: Value::EMPTY_LIST,
                traits: Value::NIL,
            }),
        );
        let lir = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                vec![
                    LirInstr::ValueConst {
                        dst: Reg(0),
                        value: inner,
                    },
                    LirInstr::ValueConst {
                        dst: Reg(1),
                        value: list,
                    },
                ],
                Terminator::Return(Reg(1)),
            )
            .build();
        // The bytecode pool holds every value a `ValueConst` loads.
        let outer = TemplateProto {
            lir_function: Some(Rc::new(lir)),
            name: Some("outer".to_string()),
            ..TemplateProto::new(Vec::new(), Arity::Exact(0), vec![inner, list])
        };
        let outer_val = crate::value::heap::alloc(
            unsafe { &mut *heap_ptr },
            HeapObject::Closure {
                closure: Closure::new(
                    crate::value::closure::test_template(unsafe { &mut *heap_ptr }, outer),
                    crate::value::region_slice::RegionSlice::empty(),
                    SignalBits::EMPTY,
                ),
                traits: Value::NIL,
            },
        );

        let bundle = SendBundle::from_value(outer_val, unsafe { &*heap_ptr }, None)
            .expect("the closure is sendable");
        let restored = into_value_in_region(|ctx| bundle.into_value(ctx, None));
        let restored = restored.as_closure().expect("a closure arrives");
        let lir = restored
            .template
            .lir()
            .expect("the LIR crosses with the closure");

        let mut loaded = Vec::new();
        for node in lir.nodes() {
            if let InstrRef::ValueConst { value, .. } = node.instr() {
                loaded.push(value);
            }
        }
        assert_eq!(loaded.len(), 2, "both ValueConsts arrive as ValueConsts");
        assert!(loaded[0].as_closure().is_some(), "the closure arrives");
        let pair = loaded[1].as_pair().expect("the list arrives");
        assert_eq!(pair.first, Value::int(1));
        assert!(pair.rest.is_empty_list());

        #[cfg(feature = "jit")]
        crate::jit::JitCompiler::new()
            .expect("a compiler")
            .compile(&lir, Vec::new())
            .expect("the worker's JIT compiles the LIR that arrived");
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
