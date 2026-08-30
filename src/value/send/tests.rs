use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{LirConst, LirFunction, LirInstr, Reg, Terminator};
use crate::value::closure::{Closure, ClosureTemplate};
use crate::value::fiber::SignalBits;
use crate::value::heap::HeapObject;
use crate::value::types::Arity;
use std::rc::Rc;

/// Reconstruct a bundle/value through a ctx over a fresh region on a leaked test
/// heap, NOT releasing the region: the result must outlive the call (the test
/// reads it afterward), so freeing the region would recycle the pages the
/// returned value points at. The leaked heap keeps it resident for the test.
fn into_value_in_region(f: impl FnOnce(&mut crate::primitives::ctx::Alloc) -> Value) -> Value {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut ctx = crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
    f(&mut ctx)
}

/// Build a minimal closure Value with an attached LIR function, on `heap`.
/// Used by the ClosureRef round-trip test.
fn make_test_closure(
    heap: *mut crate::value::fiberheap::FiberHeap,
    name: &str,
    lir: Option<LirFunction>,
) -> Value {
    let template = Rc::new(ClosureTemplate {
        num_locals: 1,
        num_params: 1,
        lir_function: lir.map(Rc::new),
        name: Some(Rc::from(name)),
        ..ClosureTemplate::new(Rc::new(vec![]), Arity::Exact(1), Rc::new(vec![]))
    });
    let closure = Closure {
        template: crate::value::TemplateRef::new(template),
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    };
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
        let outer_template = Rc::new(ClosureTemplate {
            num_captures: 1,
            lir_function: Some(Rc::new(lir)),
            name: Some(Rc::from("outer")),
            ..ClosureTemplate::new(Rc::new(vec![]), Arity::Exact(0), Rc::new(vec![]))
        });
        // Build the env slice and the closure header in ONE explicit region
        // (slice + header must share a region), on the same heap as `inner`.
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let env = crate::value::arena::alloc_region_slice_in_region::<Value>(
            unsafe { &mut *heap_ptr },
            &[inner],
            region,
        );
        let outer_closure = Closure {
            template: crate::value::TemplateRef::new(outer_template),
            // make `inner` reachable from the bundle
            env,
            squelch_mask: SignalBits::EMPTY,
        };
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
            .lir_function
            .as_ref()
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

// ── sendable parameters + stdio ports ────────────────────────────────

#[test]
fn parameter_round_trips_preserving_id_and_default() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let p = h.ctx().parameter(Value::int(7));
        let (id0, _) = p.as_parameter().expect("p is a parameter");

        let bundle = SendBundle::from_value(p, h.heap(), None)
            .expect("a parameter with a sendable default must be sendable");
        let p2 = into_value_in_region(|ctx| bundle.into_value(ctx, None));

        let (id1, def1) = p2
            .as_parameter()
            .expect("reconstructed value is a parameter");
        // Resolution is by id — the worker must see the same parameter.
        assert_eq!(
            id0, id1,
            "parameter id must be preserved across the boundary"
        );
        assert_eq!(def1.as_int(), Some(7), "default must round-trip");
    });
}

#[test]
fn stdio_port_round_trips_by_kind() {
    use crate::port::{Port, PortKind};
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        for (mk, kind) in [
            (Port::stdout as fn() -> Port, PortKind::Stdout),
            (Port::stderr as fn() -> Port, PortKind::Stderr),
            (Port::stdin as fn() -> Port, PortKind::Stdin),
        ] {
            let v = h.ctx().external("port", mk());
            let bundle =
                SendBundle::from_value(v, h.heap(), None).expect("stdio ports are sendable");
            let v2 = into_value_in_region(|ctx| bundle.into_value(ctx, None));
            let got = v2.as_external::<Port>().map(|p| p.kind());
            assert_eq!(got, Some(kind), "stdio port must reconstruct with its kind");
        }
    });
}

// ── symbol identity survives the boundary verbatim ──────────────────
//
// The three tests below cover the three ways a symbol id crosses `send`: as a
// value, as a struct key, and as a LIR constant. Only the first was ever
// translated; the other two always crossed raw, so they were correct only when
// the two tables happened to agree. Hash identity makes all three the same
// case, and each test asserts a name resolves on the receiving side.

/// A receiver's table whose history differs from the sender's. Under
/// mint-order ids the four decoys are what made every shared name disagree.
fn skewed_receiver() -> crate::symbol::SymbolTable {
    let mut receiver = crate::symbol::SymbolTable::new();
    for n in ["skew-w", "skew-x", "skew-y", "skew-z"] {
        let _ = receiver.intern(n);
    }
    receiver
}

#[test]
fn a_symbol_value_names_the_same_symbol_on_both_sides() {
    use crate::symbol::SymbolTable;

    let mut sender = SymbolTable::new();
    let _ = sender.intern("send-aaa");
    let _ = sender.intern("send-bbb");
    let begin = sender.intern("begin");

    // A symbol is immediate, so serialization allocates nothing; any heap serves.
    let sv = SendBundle::from_value(
        Value::symbol(begin),
        unsafe { &*crate::value::arena::leaked_test_heap() },
        Some(&sender),
    )
    .expect("symbol is sendable");

    let mut receiver = skewed_receiver();
    let got = {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let mut alloc =
            crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
        sv.into_value(&mut alloc, Some(&mut receiver))
    };

    let got_id = got.as_symbol().expect("reconstructed value is a symbol");
    assert_eq!(got_id, begin, "the id crosses unchanged");
    assert_eq!(receiver.name(got_id), Some("begin"));
}

// `SendValue::Struct` clones each `TableKey` verbatim, so a symbol key crosses
// as a bare id with no name beside it. The receiver must still read the key the
// sender wrote.
//
// Counter-factual: with mint-order ids this is the defect that made
// `(get (sys/join (sys/spawn (fn [] {'alpha 1}))) 'alpha)` return the wrong
// entry — `tests/elle/symbol-identity.lisp` pins the end-to-end shape.
#[test]
fn a_symbol_struct_key_names_the_same_symbol_on_both_sides() {
    use crate::symbol::SymbolTable;
    use crate::value::heap::TableKey;

    crate::value::arena::with_test_region(|| {
        let mut sender = SymbolTable::new();
        let _ = sender.intern("send-aaa");
        let alpha = sender.intern("key-alpha");
        let beta = sender.intern("key-beta");

        let h = crate::primitives::ctx::TestHeap::new();
        let mut entries = vec![
            (TableKey::Symbol(alpha), Value::int(7)),
            (TableKey::Symbol(beta), Value::int(8)),
        ];
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        let s = h.ctx().struct_from_sorted(entries);
        let bundle =
            SendBundle::from_value(s, h.heap(), Some(&sender)).expect("struct is sendable");

        let mut receiver = skewed_receiver();
        let probe = TableKey::Symbol(receiver.intern("key-alpha"));
        let got = {
            let heap_ptr = crate::value::arena::leaked_test_heap();
            let region = unsafe { (*heap_ptr).new_runtime_region() };
            let mut alloc =
                crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
            bundle.into_value(&mut alloc, Some(&mut receiver))
        };

        let entries = got.as_struct().expect("reconstructed value is a struct");
        assert_eq!(
            crate::value::types::sorted_struct_get(entries, &probe),
            Some(&crate::value::Value::int(7)),
            "the receiver's own key must find the entry the sender stored"
        );
        // `key-beta` was never interned on this side: its name reaches the
        // receiver only through the bundle's name table, from the struct-key
        // noting site (keys are cloned as `TableKey`, never routed through
        // `from_value_inner`).
        assert_eq!(
            receiver.name(beta),
            Some("key-beta"),
            "a struct key's name must cross inside the bundle"
        );
    });
}

// A `LirConst::Symbol` ships inside the live `LirFunction` with no name and no
// rewrite, so the id the sender lowered is the id the worker re-emits into its
// own constant pool.
#[test]
fn a_lir_symbol_const_names_the_same_symbol_on_both_sides() {
    use crate::lir::value_to_lir_const;
    use crate::symbol::SymbolTable;

    let mut sender = SymbolTable::new();
    let _ = sender.intern("send-aaa");
    let alpha = sender.intern("lir-alpha");

    let shipped = match value_to_lir_const(Value::symbol(alpha)) {
        Some(LirConst::Symbol(id)) => id,
        other => panic!("a symbol lowers to LirConst::Symbol, got {:?}", other),
    };

    let mut receiver = skewed_receiver();
    let _ = receiver.intern("lir-alpha");
    assert_eq!(
        receiver.name(shipped),
        Some("lir-alpha"),
        "the worker re-emits this id verbatim; it must name the sender's symbol"
    );
}

#[test]
fn parameter_holding_stdout_port_is_sendable() {
    // `*stdout*` is `(parameter (port/stdout))`; this is the exact shape a
    // `println`-using closure closes over. It must serialize.
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let p = h
            .ctx()
            .parameter(h.ctx().external("port", crate::port::Port::stdout()));
        let bundle = SendBundle::from_value(p, h.heap(), None)
            .expect("a parameter defaulting to a stdio port must be sendable");
        let p2 = into_value_in_region(|ctx| bundle.into_value(ctx, None));
        let (_, def) = p2.as_parameter().expect("reconstructed is a parameter");
        assert_eq!(
            def.as_external::<crate::port::Port>().map(|p| p.kind()),
            Some(crate::port::PortKind::Stdout),
            "the parameter's default stdout port must round-trip"
        );
    });
}

// ── an abandoned frame's release tables cross the boundary ───────────

#[test]
fn closure_round_trips_preserving_frame_release_tables() {
    // `frame_release_slots`/`frame_release_regions` are the table an error
    // exit walks to run the releases the abandoned frame still owed
    // (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
    // it still owes"). A closure keeps its body across the boundary, so it
    // keeps that obligation: reconstruct it with empty tables and every
    // region an erroring worker frame owed is stranded.
    crate::value::arena::with_test_region(|| {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let child = Rc::new(ClosureTemplate {
            frame_release_slots: Rc::new(vec![21u16]),
            frame_release_regions: Rc::new(vec![23u32]),
            ..ClosureTemplate::new(Rc::new(vec![]), Arity::Exact(0), Rc::new(vec![]))
        });
        let template = Rc::new(ClosureTemplate {
            num_locals: 1,
            num_params: 1,
            frame_release_slots: Rc::new(vec![3u16, 7]),
            frame_release_regions: Rc::new(vec![11u32, 13]),
            child_protos: Rc::new(vec![child]),
            ..ClosureTemplate::new(Rc::new(vec![]), Arity::Exact(1), Rc::new(vec![]))
        });
        let val = crate::value::heap::alloc(
            unsafe { &mut *heap_ptr },
            HeapObject::Closure {
                closure: Closure {
                    template: crate::value::TemplateRef::new(template),
                    env: crate::value::region_slice::RegionSlice::empty(),
                    squelch_mask: SignalBits::EMPTY,
                },
                traits: Value::NIL,
            },
        );

        let bundle = SendBundle::from_value(val, unsafe { &*heap_ptr }, None)
            .expect("a plain closure is sendable");
        let restored = into_value_in_region(|ctx| bundle.into_value(ctx, None));

        let closure = restored.as_closure().expect("restored value is a closure");
        assert_eq!(
            &closure.template.frame_release_slots[..],
            &[3u16, 7],
            "the value-routed release slots must cross the boundary — an empty \
             table silently strands every region an erroring worker frame owed"
        );
        assert_eq!(
            &closure.template.frame_release_regions[..],
            &[11u32, 13],
            "the slot-routed release regions must cross with them; the two \
             halves of one table are useless apart"
        );
        let child = &closure.template.child_protos[0];
        assert_eq!(
            &child.frame_release_slots[..],
            &[21u16],
            "a nested-lambda blueprint crosses via sendable_from_template and \
             template_from_sendable, not send_closure — its tables must cross too"
        );
        assert_eq!(&child.frame_release_regions[..], &[23u32]);
    });
}

// A keyword crosses as an immediate; its spelling rides the bundle's name
// table with the symbols', so the receiving instance can print both the
// keyword value and a keyword struct key it never learned itself.
#[test]
fn a_keyword_names_the_same_keyword_on_both_sides() {
    use crate::symbol::SymbolTable;
    use crate::value::heap::TableKey;

    crate::value::arena::with_test_region(|| {
        let mut sender = SymbolTable::new();
        let kw_hash = sender.keyword("kw-send-xt");
        let key_hash = sender.keyword("kw-send-key-xt");

        let h = crate::primitives::ctx::TestHeap::new();
        let s = h.ctx().struct_from_sorted(vec![(
            TableKey::Keyword(key_hash),
            Value::keyword("kw-send-xt"),
        )]);
        let bundle =
            SendBundle::from_value(s, h.heap(), Some(&sender)).expect("struct is sendable");

        let mut receiver = SymbolTable::new();
        let got = {
            let heap_ptr = crate::value::arena::leaked_test_heap();
            let region = unsafe { (*heap_ptr).new_runtime_region() };
            let mut alloc =
                crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
            bundle.into_value(&mut alloc, Some(&mut receiver))
        };

        assert_eq!(
            format!("{}", got.display_with(Some(&receiver))),
            "{:kw-send-key-xt :kw-send-xt}",
            "both the key's and the value's spellings crossed in the bundle"
        );
        assert_eq!(receiver.keyword_name(kw_hash), Some("kw-send-xt"));
    });
}

// ── the serde mirror keeps container kinds apart ────────────────────

/// `SendValue`'s serde is the stdlib cache's wire format, and eight variants
/// share three shapes: a sequence, a map, a byte run. Collapsing them loses
/// which one it was, so a `Tuple` returns as an `Array`, an `LSet` as an
/// `Array`, and a `Buffer` — a *mutable* @string — as immutable `Bytes`. The
/// value would be silently the wrong type on every reload.
#[test]
fn serde_round_trip_keeps_each_container_kind_distinct() {
    use super::SendValue as SV;

    fn nil() -> Box<SV> {
        Box::new(SV::Immediate(Value::NIL))
    }
    fn name(sv: &SV) -> &'static str {
        match sv {
            SV::Array(..) => "Array",
            SV::Tuple(..) => "Tuple",
            SV::LSet(..) => "LSet",
            SV::LSetMut(..) => "LSetMut",
            SV::Struct(..) => "Struct",
            SV::StructMut(..) => "StructMut",
            SV::Buffer(..) => "Buffer",
            SV::Bytes(..) => "Bytes",
            SV::Blob(..) => "Blob",
            _ => panic!("unexpected variant in this test"),
        }
    }

    let empty = std::collections::BTreeMap::new();
    let cases = vec![
        SV::Array(vec![], nil()),
        SV::Tuple(vec![], nil()),
        SV::LSet(vec![], nil()),
        SV::LSetMut(vec![], nil()),
        SV::Struct(empty.clone(), nil()),
        SV::StructMut(empty, nil()),
        SV::Buffer(vec![1, 2, 3], nil()),
        SV::Bytes(vec![1, 2, 3], nil()),
        SV::Blob(vec![1, 2, 3], nil()),
    ];

    for case in cases {
        let bytes = bincode::serialize(&case).expect("serializes");
        let back: SV = bincode::deserialize(&bytes).expect("deserializes");
        assert_eq!(
            name(&back),
            name(&case),
            "a {} must not come back as a {} — the cache would hand the runtime \
             a value of the wrong type on every reload",
            name(&case),
            name(&back)
        );
    }
}
