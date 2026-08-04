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
fn into_value_in_region(
    f: impl FnOnce(&mut crate::primitives::ctx::Alloc, &mut crate::symbol::SymbolTable) -> Value,
) -> Value {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut ctx = crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
    // These round-trips carry no symbols; a fresh table suffices. The
    // symbol-specific round-trip threads its own sender/receiver tables.
    let mut symbols = crate::symbol::SymbolTable::new();
    f(&mut ctx, &mut symbols)
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
        let bundle = SendBundle::from_value(
            outer_val,
            unsafe { &*heap_ptr },
            &crate::symbol::SymbolTable::new(),
        )
        .expect("should serialize");
        let restored = into_value_in_region(|ctx, symbols| bundle.into_value(ctx, symbols));

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

        let bundle = SendBundle::from_value(p, h.heap(), &crate::symbol::SymbolTable::new())
            .expect("a parameter with a sendable default must be sendable");
        let p2 = into_value_in_region(|ctx, symbols| bundle.into_value(ctx, symbols));

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
            let bundle = SendBundle::from_value(v, h.heap(), &crate::symbol::SymbolTable::new())
                .expect("stdio ports are sendable");
            let v2 = into_value_in_region(|ctx, symbols| bundle.into_value(ctx, symbols));
            let got = v2.as_external::<Port>().map(|p| p.kind());
            assert_eq!(got, Some(kind), "stdio port must reconstruct with its kind");
        }
    });
}

// ── symbol values re-intern by name (not by raw id) ─────────────────

#[test]
fn symbol_round_trips_by_name_not_id() {
    use crate::symbol::SymbolTable;
    use crate::value::SymbolId;

    // Sender's table: intern a couple of names first so "begin" lands at
    // some non-zero id that is meaningful only in THIS table.
    let mut sender = SymbolTable::new();
    let _ = sender.intern("aaa");
    let _ = sender.intern("bbb");
    let begin_sender = sender.intern("begin");

    // Serialize a symbol value carrying the sender-table id, resolving the name
    // against the sender's table (threaded explicitly).
    // A symbol is immediate, so serialization allocates nothing; any heap serves.
    let sv = SendBundle::from_value(
        Value::symbol(begin_sender.0),
        unsafe { &*crate::value::arena::leaked_test_heap() },
        &sender,
    )
    .expect("symbol is sendable");

    // Receiver's table: a DIFFERENT layout, so "begin" gets a different id.
    // This is the cross-thread reality the worker faces.
    let mut receiver = SymbolTable::new();
    for n in ["w", "x", "y", "z"] {
        let _ = receiver.intern(n);
    }
    let begin_receiver = receiver.intern("begin");
    assert_ne!(
        begin_sender.0, begin_receiver.0,
        "test setup: the two tables must assign 'begin' different ids"
    );

    // Reconstruct re-interning into the RECEIVER's table (threaded explicitly).
    // A symbol is an immediate, so reconstruction allocates nothing — any mortal
    // ctx serves as the target.
    let got = {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let mut alloc =
            crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
        sv.into_value(&mut alloc, &mut receiver)
    };

    let got_id = got.as_symbol().expect("reconstructed value is a symbol");
    assert_eq!(
        receiver.name(SymbolId(got_id)),
        Some("begin"),
        "a symbol must cross the boundary by NAME and re-intern in the \
             receiver's table — carrying the raw sender id would resolve to the \
             wrong name (or none)"
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
        let bundle = SendBundle::from_value(p, h.heap(), &crate::symbol::SymbolTable::new())
            .expect("a parameter defaulting to a stdio port must be sendable");
        let p2 = into_value_in_region(|ctx, symbols| bundle.into_value(ctx, symbols));
        let (_, def) = p2.as_parameter().expect("reconstructed is a parameter");
        assert_eq!(
            def.as_external::<crate::port::Port>().map(|p| p.kind()),
            Some(crate::port::PortKind::Stdout),
            "the parameter's default stdout port must round-trip"
        );
    });
}
