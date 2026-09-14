// audited: 2026-09-14
// The code objects a hydrated `MakeClosure` indexes: the payload's child
// table, filled by the dumper because the blueprint cannot cross.
// docs/impl/image/sealing.md
// docs/impl/image/plan.md

use std::rc::Rc;

use super::*;
use elle::image;
use elle::pipeline::eval_all;
use elle::runtime::Runtime;
use elle::value::closure::{ChildCode, ClosureTemplate};
use elle::value::fiber::SignalBits;
use elle::value::heap::deref;
use elle::value::{Arity, SendBundle, SendValue, TemplateProto};

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
            "<image-children>",
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
        let result = eval_all(call, symbols, vm, cctx, "<image-children>").expect("call");
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
        eval_all("(fn [x] (fn [] x))", symbols, vm, cctx, "<image-children>").expect("eval")
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
            "<image-children>",
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
