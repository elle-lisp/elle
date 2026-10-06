// audited: 2026-10-06
//! A payload carries its function's LIR: the blueprint's function, in the
//! payload's own pages, gone when they go.
//! docs/impl/region/template.md
//! docs/impl/lir.md

use std::rc::Rc;

use super::*;
use crate::lir::code::Node;
use crate::pipeline::compile_file;
use crate::runtime::Runtime;

/// Every nested-lambda blueprint `src` compiles to, each once, parents before
/// their children.
fn blueprints(rt: &mut Runtime, src: &str, file: &str) -> Vec<Rc<TemplateProto>> {
    let (_vm, symbols, cctx) = rt.parts();
    let result = compile_file(src, symbols, cctx, file).expect("the source compiles");
    let mut out: Vec<Rc<TemplateProto>> = Vec::new();
    let mut stack: Vec<Rc<TemplateProto>> =
        result.bytecode.child_protos.iter().rev().cloned().collect();
    while let Some(p) = stack.pop() {
        if out.iter().any(|q| Rc::ptr_eq(q, &p)) {
            continue;
        }
        stack.extend(p.child_protos.iter().rev().cloned());
        out.push(p);
    }
    out
}

/// The one lambda `src` compiles to.
fn lambda(rt: &mut Runtime, src: &str) -> Rc<TemplateProto> {
    let mut all = blueprints(rt, src, "<lir-payload>");
    assert_eq!(all.len(), 1, "the source compiles to one lambda");
    all.pop().expect("one lambda")
}

/// A blueprint carrying every field of `p` but its LIR, so a payload built
/// from it differs from `p`'s by the LIR alone.
fn without_lir(p: &TemplateProto) -> TemplateProto {
    TemplateProto {
        num_locals: p.num_locals,
        num_captures: p.num_captures,
        num_params: p.num_params,
        signal: p.signal,
        capture_params_mask: p.capture_params_mask,
        capture_locals_mask: p.capture_locals_mask.clone(),
        location_map: p.location_map.clone(),
        doc: p.doc.clone(),
        origin: p.origin,
        vararg_kind: p.vararg_kind.clone(),
        rest_list_layout: p.rest_list_layout,
        name: p.name.clone(),
        region_table: p.region_table.clone(),
        merged_slots: p.merged_slots.clone(),
        frame_release_slots: p.frame_release_slots.clone(),
        frame_release_regions: p.frame_release_regions.clone(),
        ..TemplateProto::new(p.bytecode.clone(), p.arity, p.constants.clone())
    }
}

/// Bytes of node records a blueprint's LIR holds.
fn node_bytes(p: &TemplateProto) -> usize {
    let lir = p
        .lir_function
        .as_ref()
        .expect("a nested lambda carries LIR");
    lir.view().nodes().count() * std::mem::size_of::<Node>()
}

/// Every lambda the standard library compiles reads, out of its payload, the
/// function its blueprint froze: every header field, every block, every
/// instruction with its span, and every site. The payload is what every
/// reader of a code object reads, so a field the copy loses is a field the
/// JIT compiles wrong.
///
/// The counter-factual is a payload that carries no LIR, which is what a
/// hydrated closure read before the LIR moved into the payload: the closure
/// still runs, on the interpreter, and only this answer tells.
#[test]
fn every_stdlib_lambdas_payload_answers_its_blueprints_function() {
    let mut rt = Runtime::new();
    let protos = blueprints(&mut rt, crate::pipeline::sources::STDLIB, "stdlib.lisp");
    assert!(
        protos.len() > 100,
        "the standard library compiles to {} lambdas, so the walk missed most",
        protos.len()
    );
    let mut heap = FiberHeap::new();
    for p in &protos {
        let want = p
            .lir_function
            .as_ref()
            .expect("a nested lambda's blueprint carries LIR")
            .view();
        let label = want.name().unwrap_or("<anon>").to_string();
        let t = header(header_in(&mut heap, p));
        let got = t
            .lir()
            .unwrap_or_else(|| panic!("{label}: the payload carries no LIR"));
        if let Some(diff) = want.first_difference(&got) {
            panic!("{label}: the payload's LIR differs from the blueprint's at {diff}");
        }
    }
}

/// A code object with no LIR — an entry thunk, a hand-built blueprint —
/// answers none, rather than an empty function the JIT would compile.
#[test]
fn a_payload_without_lir_answers_none() {
    let mut heap = FiberHeap::new();
    let t = header(header_in(&mut heap, &proto(vec![1, 2, 3])));
    assert!(t.lir().is_none(), "a blueprint with no LIR grew one");
}

/// A promotion copies the function out of the payload, and the copy answers
/// after the payload's region is freed and its pages written over. The JIT
/// worker reads the copy on another thread at any later time, so a copy that
/// still pointed into the pages would compile whatever the pages held then.
///
/// The counter-factual is a copy that borrows the payload's slices: it reads
/// correctly until the free lands, and this test frees it before reading.
#[test]
fn a_promotion_copy_answers_after_its_payload_region_is_freed() {
    let mut rt = Runtime::new();
    let p = lambda(&mut rt, "(fn [x] (if (%eq x 2) 40 (+ x 7)))");
    let want = Rc::clone(p.lir_function.as_ref().expect("the lambda carries LIR"));

    let mut heap = FiberHeap::new();
    let region = region(&mut heap);
    let copy = {
        let t = header(materialize(&mut heap, &p, region));
        t.lir().expect("the payload carries LIR").to_owned()
    };
    let payload_regions = heap.template_payload_regions();
    assert_eq!(
        payload_regions.len(),
        1,
        "one blueprint, one payload region"
    );

    // Free the header, then the blueprint, then the payload's region.
    heap.decref_region_if_present(region);
    drop(p);
    heap.release_dead_template_payloads();
    assert!(
        heap.template_payload_regions().is_empty(),
        "the payload region outlived its blueprint, so nothing here was freed"
    );

    // Claim the freed pages again and write over them.
    let scribble = heap.new_runtime_region();
    for _ in 0..64 {
        heap.alloc_region_slice_in_region(&[0xABu8; 4096], scribble);
    }

    if let Some(diff) = want.view().first_difference(&copy.view()) {
        panic!("the promotion copy changed when its payload was freed, at {diff}");
    }
}

/// A payload's LIR lands in the payload's region and leaves with it. The
/// region grows by at least the node records the function holds, and freeing
/// the blueprint returns the heap to the regions it had before.
///
/// The counter-factual is LIR kept beside the payload in Rust memory: the
/// region would not grow, and no region gauge could see the bytes.
#[test]
fn a_payloads_lir_lands_in_its_region_and_leaves_with_it() {
    let mut rt = Runtime::new();
    let p = lambda(&mut rt, "(fn [x] (if (%eq x 2) 40 (+ x 7)))");
    let bare = Rc::new(without_lir(&p));

    // Slices bump down from each page's end, so a page holds `len -
    // data_cursor` bytes of them. Committed bytes would count whole pages,
    // which one small function's LIR need not add.
    let grown = |p: &Rc<TemplateProto>| {
        let mut heap = FiberHeap::new();
        let region = region(&mut heap);
        materialize(&mut heap, p, region);
        let payload = heap.template_payload_regions();
        assert_eq!(payload.len(), 1, "one blueprint, one payload region");
        heap.region_pool(payload[0])
            .expect("the payload region is live")
            .page_layouts()
            .iter()
            .map(|l| l.len - l.data_cursor)
            .sum::<usize>()
    };
    let (with, without) = (grown(&p), grown(&bare));
    assert!(
        with >= without + node_bytes(&p),
        "a payload with LIR allocated {with} bytes against {without} without; \
         the {} bytes of node records are not in the region",
        node_bytes(&p)
    );

    let mut heap = FiberHeap::new();
    let baseline = heap.active_region_count();
    let region = region(&mut heap);
    materialize(&mut heap, &p, region);
    heap.decref_region_if_present(region);
    drop(p);
    heap.release_dead_template_payloads();
    assert_eq!(
        heap.active_region_count(),
        baseline,
        "freeing the header and its blueprint left a region behind"
    );
}

/// The standard library's LIR is region pages the `arena/page-claims` gauge
/// counts: materializing every stdlib lambda claims more pages than
/// materializing the same code objects without their LIR.
#[test]
fn the_stdlib_lir_shows_in_page_claims() {
    let mut rt = Runtime::new();
    let protos = blueprints(&mut rt, crate::pipeline::sources::STDLIB, "stdlib.lisp");
    let claims = |protos: &[Rc<TemplateProto>]| {
        let mut heap = FiberHeap::new();
        let before = heap.page_claims();
        for p in protos {
            header_in(&mut heap, p);
        }
        heap.page_claims() - before
    };
    let bare: Vec<Rc<TemplateProto>> = protos.iter().map(|p| Rc::new(without_lir(p))).collect();
    let (with, without) = (claims(&protos), claims(&bare));
    assert!(
        with > without,
        "the stdlib's payloads claimed {with} pages with their LIR and \
         {without} without, so the LIR is not in region pages"
    );
}
