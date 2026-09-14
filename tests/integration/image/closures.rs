// audited: 2026-09-14
// A closure and its code object cross the body; the header hydrates without
// its blueprint.
// docs/impl/image/sealing.md
// docs/impl/image/plan.md

use std::rc::Rc;

use super::*;
use elle::hir::region::StaticRegion;
use elle::hir::VarargKind;
use elle::image::{self, ImageError, Sections};
use elle::pipeline::eval_all;
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::closure::{materialize, ChildCode, ClosureTemplate};
use elle::value::fiber::SignalBits;
use elle::value::heap::{deref, Closure};
use elle::value::{Arity, CaptureMask, SendBundle, SendValue, TemplateProto, TemplateRef};
use elle::SourceLoc;

/// A blueprint exercising every payload field a data closure carries: real
/// locations with two interned files, both release tables, a merge set, the
/// capture masks, a `&named` key set, and a mixed constant pool.
fn full_proto(heap: &mut FiberHeap, region: RuntimeRegion) -> (TemplateProto, Value) {
    let shared = alloc_str(heap, region, "shared payload");
    let insert = Value::native_fn(
        elle::primitives::prim_table_snapshot()
            .into_iter()
            .find(|d| d.name == "insert")
            .expect("insert is a canonical primitive"),
    );
    let mut proto = TemplateProto::new(
        vec![7, 1, 4, 1, 9],
        Arity::AtLeast(1),
        vec![Value::int(9), shared, insert, Value::keyword("payload-kw")],
    );
    proto.num_locals = 3;
    proto.num_captures = 2;
    proto.num_params = 2;
    proto.signal = Signal::errors();
    proto.capture_params_mask = 0b10;
    proto.capture_locals_mask = CaptureMask::from_words(vec![0b100]);
    proto
        .location_map
        .insert(0, SourceLoc::new("closure-a.lisp", 3, 9));
    proto
        .location_map
        .insert(2, SourceLoc::new("closure-b.lisp", 14, 1));
    proto.name = Some("image-closure".to_string());
    proto.doc = Some("crosses the body".to_string());
    proto.vararg_kind = VarargKind::StrictStruct(vec!["alpha".to_string(), "beta".to_string()]);
    proto.region_table = vec![
        StaticRegion::new(2).expect("slot 2 is a slot"),
        StaticRegion::new(4).expect("slot 4 is a slot"),
    ];
    proto.merged_slots.extend([5u32, 2]);
    proto.frame_release_slots = vec![4, 1];
    proto.frame_release_regions = vec![9, 3];
    proto.origin = Some(elle::syntax::Span::synthetic());
    (proto, shared)
}

/// A closure over `proto`, its env holding `env_vals`, allocated in `region`.
fn closure_in(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    proto: &Rc<TemplateProto>,
    env_vals: &[Value],
    squelch: SignalBits,
) -> Value {
    let template = TemplateRef::region(materialize(heap, proto, region));
    let env = heap.alloc_region_slice_in_region(env_vals, region);
    heap.alloc_in_region(
        HeapObject::Closure {
            closure: Closure::new(template, env, squelch),
            traits: Value::NIL,
        },
        region,
    )
}

fn closure_of(v: Value) -> &'static Closure {
    let HeapObject::Closure { closure, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not a closure");
    };
    closure
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
    let (proto, shared) = full_proto(&mut src, region);
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(proto),
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

// A hydrated header has no blueprint, so the blueprint-only answers are
// absence. The counter-factual is the source header, which answers `origin`
// — only the dump can have dropped it.
#[test]
fn a_hydrated_header_has_no_blueprint() {
    let dir = crate::common::ScratchDir::new("image-closure-blueprint");
    let path = dir.join("closure.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let (proto, _) = full_proto(&mut src, region);
    let root = closure_in(&mut src, region, &Rc::new(proto), &[], SignalBits::EMPTY);
    assert!(
        closure_of(root).template.origin().is_some(),
        "the source header answers origin"
    );
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let t = &closure_of(hydrated.root).template;
    assert!(t.origin().is_none(), "a hydrated header answers no origin");
    assert!(t.lir_function().is_none(), "a hydrated header has no LIR");
    assert!(t.child_protos().is_empty());
}

// § Test plan, "Closures": two headers materialized from one blueprint
// hydrate naming one payload copy. The counter-factual is a per-header deep
// copy, which round-trips structurally equal and silently doubles every
// payload — only pointer identity can see it.
#[test]
fn two_headers_from_one_blueprint_hydrate_sharing_one_payload() {
    let dir = crate::common::ScratchDir::new("image-closure-payload");
    let path = dir.join("pair.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let proto = Rc::new(TemplateProto::new(
        vec![3, 1, 4],
        Arity::Exact(1),
        vec![Value::int(6)],
    ));
    let a = closure_in(&mut src, region, &proto, &[], SignalBits::EMPTY);
    let b = closure_in(&mut src, region, &proto, &[], SignalBits::EMPTY);
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
    let proto = Rc::new(TemplateProto::new(
        vec![3, 1, 4],
        Arity::Exact(1),
        vec![Value::int(6)],
    ));
    let a = closure_in(&mut src, region, &proto, &[], SignalBits::EMPTY);
    let b = closure_in(&mut src, region, &proto, &[], SignalBits::EMPTY);
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

// ── Children ────────────────────────────────────────────────────────

/// A blueprint whose one `MakeClosure` indexes `child`.
fn parent_of(name: &str, child: TemplateProto) -> TemplateProto {
    let mut proto = TemplateProto::new(vec![1], Arity::Exact(0), Vec::new());
    proto.name = Some(name.to_string());
    proto.child_protos = vec![Rc::new(child)];
    proto
}

/// The child code object `t`'s `MakeClosure` at `idx` builds, which a
/// hydrated header answers out of the body.
fn child_header(t: &ClosureTemplate, idx: usize) -> ClosureTemplate {
    match t.child(idx) {
        ChildCode::Header(child) => child,
        ChildCode::Blueprint(_) => panic!("a hydrated header answers with a blueprint"),
    }
}

// § Test plan, "Children": the child's payload crosses field by field, and a
// lambda nested two deep rides the child's own child table.
#[test]
fn a_child_code_object_crosses_field_by_field() {
    let dir = crate::common::ScratchDir::new("image-child-fields");
    let path = dir.join("children.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let mut grandchild = TemplateProto::new(vec![3, 3], Arity::Exact(2), Vec::new());
    grandchild.name = Some("grandchild".to_string());
    let mut child = parent_of("nested-lambda", grandchild);
    child.bytecode = vec![2, 2, 2];
    child.arity = Arity::Exact(1);
    child.constants = vec![Value::int(11)];
    child.doc = Some("the lambda the parent builds".to_string());
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(parent_of("parent-of-lambdas", child)),
        &[],
        SignalBits::EMPTY,
    );
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let t = &closure_of(hydrated.root).template;
    assert_eq!(t.num_children(), 1, "the parent lost its child table");

    let child = child_header(t, 0);
    assert_eq!(child.bytecode(), &[2, 2, 2]);
    assert_eq!(child.arity(), Arity::Exact(1));
    assert_eq!(child.name(), Some("nested-lambda"));
    assert_eq!(child.doc(), Some("the lambda the parent builds"));
    assert_eq!(child.constants(), &[Value::int(11)]);

    let grand = child_header(&child, 0);
    assert_eq!(grand.bytecode(), &[3, 3]);
    assert_eq!(grand.arity(), Arity::Exact(2));
    assert_eq!(grand.name(), Some("grandchild"));
    assert_eq!(grand.num_children(), 0);
}

// § Test plan, "Children": a hydrated closure builds its nested lambda, and
// the lambda answers a call. The binding is a REPL binding, so `MakeClosure`
// runs on the ordinary dispatch path and reads the child out of the body.
//
// The inner lambda captures the outer parameter, so the call also proves the
// env a hydrated `MakeClosure` builds reaches the child's frame.
#[test]
fn a_hydrated_closure_builds_its_nested_lambda() {
    let dir = crate::common::ScratchDir::new("image-child-call");
    let path = dir.join("nested.image");

    let mut rt = Runtime::new();
    let f = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(
            "(fn [x] (fn [y] (if (%eq x y) 42 7)))",
            symbols,
            vm,
            cctx,
            "<image-closures>",
        )
        .expect("eval")
    };
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    bind_hydrated(&mut rt2, &path);
    for (call, want) in [("((hydrated-f 2) 2)", 42), ("((hydrated-f 2) 5)", 7)] {
        let (vm, symbols, cctx) = rt2.parts();
        let result = eval_all(call, symbols, vm, cctx, "<image-closures>").expect("call");
        assert_eq!(result.as_int(), Some(want), "{call} answered wrong");
    }
}

// § Test plan, "Children": the instruction materializes a fresh header per
// creation. The counter-factual is handing out the image's own header, which
// answers every call correctly and quietly moves the instance-to-template
// edge across regions — only pointer identity sees the difference.
#[test]
fn two_lambdas_from_one_hydrated_parent_are_two_headers_over_one_payload() {
    let dir = crate::common::ScratchDir::new("image-child-headers");
    let path = dir.join("nested.image");

    let mut rt = Runtime::new();
    let f = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all("(fn [x] (fn [] x))", symbols, vm, cctx, "<image-closures>").expect("eval")
    };
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    bind_hydrated(&mut rt2, &path);
    // Both lambdas in one result, so neither outlives the region its call
    // returned it in.
    let both = {
        let (vm, symbols, cctx) = rt2.parts();
        eval_all(
            "(list (hydrated-f 1) (hydrated-f 1))",
            symbols,
            vm,
            cctx,
            "<image-closures>",
        )
        .expect("call")
    };
    let HeapObject::Pair(head) = (unsafe { deref(both) }) else {
        panic!("the result is not a list");
    };
    let HeapObject::Pair(tail) = (unsafe { deref(head.rest) }) else {
        panic!("the result holds one element");
    };
    let (a, b) = (closure_of(head.first), closure_of(tail.first));
    assert_ne!(
        a.template.value().as_heap_ptr(),
        b.template.value().as_heap_ptr(),
        "the two lambdas share one header, so the instruction handed out the \
         image's own rather than materializing a fresh one"
    );
    assert_eq!(
        a.template.bytecode().as_ptr(),
        b.template.bytecode().as_ptr(),
        "the two headers name two payloads"
    );
}

// § Test plan, "Children": a hydrated closure sent to a worker carries its
// children, which the worker rebuilds as the blueprints its own `MakeClosure`
// indexes. Without them the worker's instruction indexes an empty table and
// has nothing to build.
#[test]
fn a_hydrated_closure_sends_its_children() {
    let dir = crate::common::ScratchDir::new("image-child-send");
    let path = dir.join("children.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let mut child = TemplateProto::new(vec![2, 2, 2], Arity::Exact(1), Vec::new());
    child.name = Some("nested-lambda".to_string());
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(parent_of("parent-of-lambdas", child)),
        &[],
        SignalBits::EMPTY,
    );
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let bundle = SendBundle::from_value(hydrated.root, &dst, None).expect("a closure is sendable");
    let SendValue::Ref(idx) = bundle.root else {
        panic!("a closure bundle roots at an interned closure");
    };
    let sent = &bundle.closures[idx];
    assert_eq!(sent.child_protos.len(), 1, "the sent closure lost its child");
    assert_eq!(sent.child_protos[0].bytecode, vec![2, 2, 2]);
    assert_eq!(sent.child_protos[0].name.as_deref(), Some("nested-lambda"));
}

// § Test plan, "Children": a parent and its child write one file across two
// dumps. A child payload is assembled from the same probed extents its
// parent's is, so its construction temporaries are residue the same way.
#[test]
fn a_child_bearing_dump_is_byte_deterministic() {
    let dir = crate::common::ScratchDir::new("image-child-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let mut child = TemplateProto::new(vec![2, 2, 2], Arity::Exact(1), vec![Value::int(11)]);
    child.name = Some("nested-lambda".to_string());
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(parent_of("parent-of-lambdas", child)),
        &[],
        SignalBits::EMPTY,
    );
    paint_stack(0xAA, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &a).expect("dump a");
    paint_stack(0x55, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &b).expect("dump b");
    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "two dumps of one nested closure differ"
    );
}

// ── Refusals ────────────────────────────────────────────────────────

/// Dump `root` and expect a refusal whose message contains `named`.
fn refused(src: &mut FiberHeap, root: Value, path: &std::path::Path, named: &str) {
    match image::dump(src, &SymbolTable::new(), root, path) {
        Err(ImageError::Unsupported(what)) => assert!(
            what.contains(named),
            "the refusal does not name {named}: {what}"
        ),
        other => panic!("expected a refused dump, got {other:?}"),
    }
    assert!(!path.exists(), "a refused dump left a partial file");
}

// A child a compiled WASM module built refuses the dump where any other WASM
// closure does. Being a child buys it nothing: its dispatch index still names
// a function table of the module this process holds.
#[test]
fn a_wasm_child_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-child-wasm");
    let path = dir.join("children.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let mut child = TemplateProto::new(vec![2], Arity::Exact(0), Vec::new());
    child.name = Some("wasm-borne-child".to_string());
    child.wasm_func_idx = Some(3);
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(parent_of("parent-of-lambdas", child)),
        &[],
        SignalBits::EMPTY,
    );
    refused(&mut src, root, &path, "wasm-borne-child");
}

// A closure a compiled WASM module built dispatches through a function-table
// index of the module this process holds, so it fails the dump by name.
#[test]
fn a_wasm_closure_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-closure-wasm");
    let path = dir.join("wasm.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let mut proto = TemplateProto::new(vec![1], Arity::Exact(0), Vec::new());
    proto.name = Some("wasm-borne".to_string());
    proto.wasm_func_idx = Some(3);
    let root = closure_in(&mut src, region, &Rc::new(proto), &[], SignalBits::EMPTY);
    refused(&mut src, root, &path, "wasm-borne");
}

// A capture cell in an env still refuses: snapping is the boot milestone's
// answer, and until it lands a mutable capture is not sealed data.
#[test]
fn an_env_capture_cell_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-closure-cell");
    let path = dir.join("cell.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let cell = src.alloc_in_region(
        HeapObject::CaptureCell {
            cell: Rc::new(std::cell::RefCell::new(Value::int(1))),
            traits: Value::NIL,
        },
        region,
    );
    let proto = Rc::new(TemplateProto::new(vec![1], Arity::Exact(0), Vec::new()));
    let root = closure_in(&mut src, region, &proto, &[cell], SignalBits::EMPTY);
    refused(&mut src, root, &path, "CaptureCell");
}

// ── Determinism and hygiene ─────────────────────────────────────────

// § Test plan, "Closures": a dumped closure writes one file across two dumps.
// The payload is the widest record the dumper assembles — a struct of twelve
// slice headers and a `repr(Rust)` arity — so its construction temporaries
// are exactly where residue would come from.
#[test]
fn a_closure_dump_is_byte_deterministic() {
    let dir = crate::common::ScratchDir::new("image-closure-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let (proto, shared) = full_proto(&mut src, region);
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(proto),
        &[shared],
        SignalBits::EMPTY,
    );
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
// tears down like any other region, blueprint-less headers included.
#[test]
fn freeing_a_hydrated_closure_region_returns_to_baseline() {
    let dir = crate::common::ScratchDir::new("image-closure-hygiene");
    let path = dir.join("closure.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let (proto, shared) = full_proto(&mut src, region);
    let root = closure_in(
        &mut src,
        region,
        &Rc::new(proto),
        &[shared],
        SignalBits::EMPTY,
    );
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

/// Hydrate `path` into `rt` and bind its root as `hydrated-f`, so a call
/// reaches it through the ordinary dispatch path. Answers the root.
fn bind_hydrated(rt: &mut Runtime, path: &std::path::Path) -> Value {
    let hydrated = {
        let (heap, symbols) = rt.heap_and_symbols();
        image::hydrate_path(heap, symbols, path).expect("hydrate")
    };
    let (signal, arity) = {
        let closure = hydrated
            .root
            .as_closure()
            .expect("the hydrated root is a closure");
        (closure.template.signal(), closure.template.arity())
    };
    let sym = rt.symbols().intern("hydrated-f");
    {
        let (cctx, heap) = rt.compile_and_heap();
        cctx.register_repl_binding(heap, sym, hydrated.root, signal, Some(arity));
    }
    hydrated.root
}

// § Test plan, "Closures": a closure compiled in one runtime answers a call
// in a fresh one, through a REPL binding and the ordinary dispatch path. The
// source closure carries LIR (every compiled lambda does); the hydrated one
// carries none, so the call below is also the interpreter-tier pin.
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
    assert!(
        f.as_closure()
            .expect("the eval produced a closure")
            .template
            .lir_function()
            .is_some(),
        "a compiled lambda carries LIR before the dump"
    );
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    let root = bind_hydrated(&mut rt2, &path);
    assert!(
        root.as_closure()
            .expect("the hydrated root is a closure")
            .template
            .lir_function()
            .is_none(),
        "a hydrated closure runs the interpreter tier"
    );
    let result = {
        let (vm, symbols, cctx) = rt2.parts();
        eval_all("(hydrated-f 2)", symbols, vm, cctx, "<image-closures>").expect("call")
    };
    assert_eq!(result.as_int(), Some(42), "the hydrated closure answered wrong");
}
