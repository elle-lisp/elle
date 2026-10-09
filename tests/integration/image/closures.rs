// audited: 2026-10-06
// A closure and its code object cross the body, field by field and sharing
// kept.
// docs/impl/image/sealing.md
// docs/impl/image/plan.md

use std::rc::Rc;

use super::*;
use elle::hir::region::StaticRegion;
use elle::hir::VarargKind;
use elle::image::Sections;
use elle::pipeline::eval_all;
use elle::signals::Signal;
use elle::value::heap::deref;
use elle::value::{Arity, CaptureMask, RestListLayout};
use elle::SourceLoc;

/// A code object exercising every payload field a data closure carries: real
/// locations with two interned files, both release tables, a merge set, the
/// capture masks, a `&named` key set, the rest-list layout, and a mixed
/// constant pool. Answers the code object and its shared string constant.
fn full_code(heap: &mut FiberHeap, region: RuntimeRegion) -> (TemplateRef, Value) {
    let shared = alloc_str(heap, region, "shared payload");
    let insert = Value::native_fn(
        elle::primitives::prim_table_snapshot()
            .into_iter()
            .find(|d| d.name == "insert")
            .expect("insert is a canonical primitive"),
    );
    let mut locations = elle::error::LocationMap::new();
    locations.insert(0, SourceLoc::new("closure-a.lisp", 3, 9));
    locations.insert(2, SourceLoc::new("closure-b.lisp", 14, 1));
    let code = CodeBuilder::new(
        vec![7, 1, 4, 1, 9],
        Arity::AtLeast(1),
        vec![Value::int(9), shared, insert, Value::keyword("payload-kw")],
    )
    .num_locals(3)
    .num_captures(2)
    .num_params(2)
    .signal(Signal::errors())
    .capture_params_mask(0b10)
    .capture_locals_mask(CaptureMask::from_words(vec![0b100]))
    .location_map(locations)
    .name("image-closure")
    .doc("crosses the body")
    .vararg_kind(VarargKind::StrictStruct(vec![
        "alpha".to_string(),
        "beta".to_string(),
    ]))
    .rest_list_layout(RestListLayout::OneRegion)
    .region_table(vec![
        StaticRegion::new(2).expect("slot 2 is a slot"),
        StaticRegion::new(4).expect("slot 4 is a slot"),
    ])
    .merged_slots(vec![5, 2])
    .frame_release_slots(vec![4, 1])
    .frame_release_regions(vec![9, 3])
    .origin(elle::syntax::Span::synthetic())
    .build(heap);
    (code, shared)
}

// § Test plan, "Closures": the payload survives field by field, and the env
// and squelch mask beside it. Sharing is one graph: an env value that is also
// a constant hydrates as ONE object, reachable both ways.
#[test]
fn a_closures_code_object_round_trips_field_by_field() {
    let dir = crate::common::ScratchDir::new("image-closure-fields");
    let path = dir.join("closure.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let (code, shared) = full_code(&mut src, region);
    let root = closure_in(
        &mut src,
        region,
        &code,
        &[shared, Value::int(5)],
        SignalBits::from_bit(33),
    );
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let closure = closure_of(hydrated.root);
    let t = &closure.template;

    assert_eq!(t.bytecode(), &[7, 1, 4, 1, 9]);
    assert_eq!(t.arity(), Arity::AtLeast(1));
    assert_eq!(t.signal(), Signal::errors());
    assert_eq!(t.num_locals(), 3);
    assert_eq!(t.num_captures(), 2);
    assert_eq!(t.num_params(), 2);
    assert_eq!(t.capture_params_mask(), 0b10);
    assert!(t.capture_locals_mask().is_set(2));
    assert!(!t.capture_locals_mask().is_set(1));
    assert_eq!(t.name(), Some("image-closure"));
    assert_eq!(t.doc(), Some("crosses the body"));
    assert_eq!(
        t.locations().get(0),
        Some(SourceLoc::new("closure-a.lisp", 3, 9))
    );
    assert_eq!(
        t.locations().get(2),
        Some(SourceLoc::new("closure-b.lisp", 14, 1))
    );
    assert!(t.merged_slots().contains(2) && t.merged_slots().contains(5));
    assert!(!t.merged_slots().contains(3));
    assert_eq!(t.frame_release_slots(), &[1, 4]);
    assert_eq!(t.frame_release_regions(), &[3, 9]);
    assert_eq!(
        t.region_table(),
        &[StaticRegion::new(2).unwrap(), StaticRegion::new(4).unwrap()]
    );
    assert!(t.strict_keys().contains("alpha") && t.strict_keys().contains("beta"));
    assert!(!t.strict_keys().contains("gamma"));
    assert_eq!(t.rest_list_layout(), RestListLayout::OneRegion);

    let constants = t.constants();
    assert_eq!(constants[0], Value::int(9));
    assert_eq!(constants[3], Value::keyword("payload-kw"));
    assert_eq!(
        constants[2].as_native_def().map(|d| d.name),
        Some("insert"),
        "the native-fn constant does not resolve by name"
    );

    let env = closure.env.as_slice();
    assert_eq!(env.len(), 2);
    assert_eq!(env[1], Value::int(5));
    assert_eq!(
        env[0].as_heap_ptr(),
        constants[1].as_heap_ptr(),
        "the env value and the constant hydrated as two objects"
    );
    assert_eq!(closure.squelch_mask, SignalBits::from_bit(33));
}

// § Test plan, "Closures": two headers over one payload hydrate naming one
// payload copy. The counter-factual is a per-header deep copy, which
// round-trips structurally equal and silently doubles every payload — only
// pointer identity can see it.
#[test]
fn two_headers_over_one_payload_hydrate_sharing_one_payload() {
    let dir = crate::common::ScratchDir::new("image-closure-payload");
    let path = dir.join("pair.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let code =
        CodeBuilder::new(vec![3, 1, 4], Arity::Exact(1), vec![Value::int(6)]).build(&mut src);
    let a = closure_in(&mut src, region, &code, &[], SignalBits::EMPTY);
    let b = closure_in(&mut src, region, &code, &[], SignalBits::EMPTY);
    assert_eq!(
        closure_of(a).template.bytecode().as_ptr(),
        closure_of(b).template.bytecode().as_ptr(),
        "the source headers share one payload"
    );
    let root = alloc_pair(&mut src, region, a, b);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let HeapObject::Pair(pair) = (unsafe { deref(hydrated.root) }) else {
        panic!("the hydrated root is not a pair");
    };
    let (ha, hb) = (closure_of(pair.first), closure_of(pair.rest));
    assert_ne!(
        pair.first.as_heap_ptr(),
        pair.rest.as_heap_ptr(),
        "the two closures collapsed into one"
    );
    assert_eq!(
        ha.template.bytecode().as_ptr(),
        hb.template.bytecode().as_ptr(),
        "the hydrated headers each carry a payload copy of their own"
    );
    assert_eq!(ha.template.bytecode(), &[3, 1, 4]);
}

// § Test plan, "Closures": the relocation stream records a shared payload's
// slots once, however many headers name it. The counter-factual is a
// per-header walk, which appends the payload's inner entries again for the
// second header — exact duplicates, adjacent once the stream is sorted.
#[test]
fn a_shared_payloads_slots_relocate_once() {
    let dir = crate::common::ScratchDir::new("image-closure-reloc-once");
    let path = dir.join("pair.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let code =
        CodeBuilder::new(vec![3, 1, 4], Arity::Exact(1), vec![Value::int(6)]).build(&mut src);
    let a = closure_in(&mut src, region, &code, &[], SignalBits::EMPTY);
    let b = closure_in(&mut src, region, &code, &[], SignalBits::EMPTY);
    let root = alloc_pair(&mut src, region, a, b);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let bytes = std::fs::read(&path).expect("read image");
    let s = image::sections(&bytes).expect("a freshly dumped image parses");
    let entry = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
    let entries: Vec<(u64, u64)> = s
        .relocations
        .clone()
        .step_by(Sections::RELOC_BYTES)
        .map(|e| (entry(e), entry(e + 8)))
        .collect();
    for pair in entries.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "the relocation stream repeats an entry, so the shared payload \
             was walked once per header"
        );
    }
}

// ── Refusals ────────────────────────────────────────────────────────

// A closure a compiled WASM module built dispatches through a function-table
// index of the module this process holds, so it fails the dump by name.
#[test]
fn a_wasm_closure_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-closure-wasm");
    let path = dir.join("wasm.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let code = CodeBuilder::new(vec![1], Arity::Exact(0), Vec::new())
        .name("wasm-borne")
        .wasm_func_idx(3)
        .build(&mut src);
    let root = closure_in(&mut src, region, &code, &[], SignalBits::EMPTY);
    refused(&mut src, root, &path, "wasm-borne");
}

// A run-time capture cell in an env still refuses: it records no binding, so
// the snapping rule has nothing to snap it as (snapping.rs owns the snaps).
#[test]
fn an_env_capture_cell_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-closure-cell");
    let path = dir.join("cell.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let cell = src.alloc_in_region(
        HeapObject::CaptureCell {
            cell: Rc::new(std::cell::RefCell::new(Value::int(1))),
            origin: elle::value::heap::CellOrigin::Runtime,
            traits: Value::NIL,
        },
        region,
    );
    let code = CodeBuilder::new(vec![1], Arity::Exact(0), Vec::new()).build(&mut src);
    let root = closure_in(&mut src, region, &code, &[cell], SignalBits::EMPTY);
    refused(&mut src, root, &path, "CaptureCell");
}

// ── Determinism and hygiene ─────────────────────────────────────────

// § Test plan, "Closures": a dumped closure writes one file across two dumps.
// The payload is the widest record the dumper assembles — a struct of
// slice headers and a `repr(Rust)` arity — so its construction temporaries
// are exactly where residue would come from.
#[test]
fn a_closure_dump_is_byte_deterministic() {
    let dir = crate::common::ScratchDir::new("image-closure-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let (code, shared) = full_code(&mut src, region);
    let root = closure_in(&mut src, region, &code, &[shared], SignalBits::EMPTY);
    paint_stack(0xAA, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &a).expect("dump a");
    paint_stack(0x55, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &b).expect("dump b");
    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "two dumps of one closure differ"
    );
}

// Hydrate, free, and the store returns to baseline: a hydrated closure region
// tears down like any other region, its headers included.
#[test]
fn freeing_a_hydrated_closure_region_returns_to_baseline() {
    let dir = crate::common::ScratchDir::new("image-closure-hygiene");
    let path = dir.join("closure.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let (code, shared) = full_code(&mut src, region);
    let root = closure_in(&mut src, region, &code, &[shared], SignalBits::EMPTY);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let before = dst.active_region_count();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    dst.decref_region_if_present(hydrated.region);
    assert_eq!(
        dst.active_region_count(),
        before,
        "a hydrated closure region left something behind"
    );
}

// ── The call: a compiled closure answers after hydration ────────────

// § Test plan, "Closures": a closure compiled in one runtime answers a call
// in a fresh one, through a REPL binding and the ordinary dispatch path. The
// source closure carries LIR (every compiled lambda does), and the hydrated
// one reads the same function out of the image's pages — the LIR the JIT
// promotes it from.
//
// The trap is the body's spelling. A stdlib wrapper like `+` is itself a
// closure the lambda captures, and stdlib closures build nested lambdas —
// which the dump refuses. `%eq` and `if` compile to bare instructions, so
// this closure's graph is its own.
#[test]
fn a_compiled_closure_answers_a_call_after_hydration() {
    let dir = crate::common::ScratchDir::new("image-closure-call");
    let path = dir.join("compiled.image");

    let mut rt = Runtime::new();
    let f = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(
            "(fn [x] (if (%eq x 2) 42 7))",
            symbols,
            vm,
            cctx,
            "<image-closures>",
        )
        .expect("eval")
    };
    let want = f
        .as_closure()
        .expect("the eval produced a closure")
        .template
        .lir()
        .expect("a compiled lambda carries LIR before the dump");
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    let root = bind_hydrated(&mut rt2, &path);
    let got = root
        .as_closure()
        .expect("the hydrated root is a closure")
        .template
        .lir()
        .expect("a hydrated closure carries its LIR");
    if let Some(diff) = want.first_difference(&got) {
        panic!("the hydrated LIR differs from the source's at {diff}");
    }
    let result = {
        let (vm, symbols, cctx) = rt2.parts();
        eval_all("(hydrated-f 2)", symbols, vm, cctx, "<image-closures>").expect("call")
    };
    assert_eq!(
        result.as_int(),
        Some(42),
        "the hydrated closure answered wrong"
    );
}

// § Test plan, "lir-payload": a hydrated closure sent to a worker carries its
// LIR, read out of the image's pages, so the worker's JIT can compile what
// arrives. The counter-factual is a sender that drops the LIR: the worker would
// run the closure interpreted with every answer still right.
#[test]
fn a_hydrated_closure_sends_its_lir() {
    use elle::lir::code::Op;
    use elle::lir::{LirOwned, LirView};
    use elle::value::{SendBundle, SendValue};

    let dir = crate::common::ScratchDir::new("image-closure-send-lir");
    let path = dir.join("compiled.image");
    let ops = |v: &LirView<'_>| v.nodes().map(|n| n.op()).collect::<Vec<Op>>();

    let mut rt = Runtime::new();
    let f = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(
            "(fn [x] (if (%eq x 2) 42 7))",
            symbols,
            vm,
            cctx,
            "<image-closures>",
        )
        .expect("eval")
    };
    let want = ops(&f
        .as_closure()
        .expect("the eval produced a closure")
        .template
        .lir()
        .expect("a compiled lambda carries LIR"));
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let bundle = SendBundle::from_value(hydrated.root, &dst, None).expect("a closure is sendable");
    let SendValue::Ref(idx) = bundle.root else {
        panic!("a closure bundle roots at an interned closure");
    };
    let sent = &bundle.closures[idx];
    let code = sent.lir.clone().expect("the sent closure lost its LIR");
    let arrived = LirOwned::from_parts(code, vec![Value::NIL; sent.lir_values.len()])
        .expect("the sent LIR names every value it carries");
    assert_eq!(
        ops(&arrived.view()),
        want,
        "the sent LIR runs different instructions"
    );
}
