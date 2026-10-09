// audited: 2026-10-06
//! A payload carries its function's LIR: the function freezing produced, in
//! its compile unit's code region, gone when that region goes.
//! docs/impl/region/template.md
//! docs/impl/lir.md

use super::*;
use crate::lir::code::Node;
use crate::lir::{Emitter, FrozenModule, LirView};
use crate::value::CodeArena;

/// `src` frozen once, and emitted from that one freeze into a code region of
/// `rt`'s heap: the module the payloads were written from, and the unit they
/// were written into.
///
/// One freeze serves both sides because two compiles of one source do not
/// agree on region slots, which come from a process-wide counter.
fn frozen_and_emitted(rt: &mut Runtime, src: &str, file: &str) -> (FrozenModule, CodeUnit) {
    let module = {
        let (_vm, symbols, cctx) = rt.parts();
        crate::pipeline::compile_file_to_lir(src, symbols, cctx, file, 0)
            .expect("the source freezes")
    };
    let code = CodeArena::mint(rt.heap());
    let (bytecode, _, _) = Emitter::new(code).emit_module(&module);
    (module, CodeUnit::new(code, bytecode))
}

/// Every nested lambda's LIR the unit's child tables reach, each once, with
/// its sites cleared: the sites are what emission adds, so a frozen module
/// has none to compare them with.
fn lambdas(unit: &CodeUnit) -> Vec<crate::lir::LirOwned> {
    fn walk(t: &ClosureTemplate, out: &mut Vec<crate::lir::LirOwned>) {
        let mut owned = t.lir().expect("a nested lambda carries LIR").to_owned();
        owned.set_sites(&[], &[]);
        out.push(owned);
        for i in 0..t.num_children() {
            walk(&t.child(i), out);
        }
    }
    let mut out = Vec::new();
    let entry = unit.entry();
    for i in 0..entry.num_children() {
        walk(&entry.child(i), &mut out);
    }
    out
}

/// A lambda with a branch, for the tests that need a function with more than
/// one block. `numeric!` proves `x` for the intrinsics, which a runtime with
/// no stdlib has no wrapper for.
const LAMBDA: &str = "(fn [x] (numeric!) (if (%eq x 2) 40 (%add x 7)))";

/// Bytes of node records `lir` holds.
fn node_bytes(lir: &LirView<'_>) -> usize {
    lir.nodes().count() * std::mem::size_of::<Node>()
}

/// Bytes of slices written into `region`'s pages. Slices bump down from each
/// page's end, so a page holds `len - data_cursor` bytes of them; committed
/// bytes would count whole pages, which one small function's LIR need not
/// add.
fn slice_bytes(heap: &FiberHeap, region: RuntimeRegion) -> usize {
    heap.region_pool(region)
        .expect("the region is live")
        .page_layouts()
        .iter()
        .map(|l| l.len - l.data_cursor)
        .sum()
}

/// Every lambda the standard library compiles reads, out of its payload, the
/// function freezing produced: every header field, every block, every
/// instruction with its span. The payload is what every reader of a code
/// object reads, so a field the emitter leaves out is a field the JIT compiles
/// wrong.
///
/// The counter-factual is a payload written without its LIR, which is what a
/// hydrated closure read before the LIR moved into the payload: the closure
/// still runs, on the interpreter, and only this answer tells.
#[test]
fn every_stdlib_lambdas_payload_answers_its_frozen_function() {
    let mut rt = Runtime::new();
    let (module, unit) =
        frozen_and_emitted(&mut rt, crate::pipeline::sources::STDLIB, "stdlib.lisp");
    let got = lambdas(&unit);
    assert!(
        got.len() > 100,
        "the standard library compiles to {} lambdas, so the walk missed most",
        got.len()
    );
    for lir in &got {
        let lir = lir.view();
        let id = lir.closure_id().expect("a nested lambda has a closure id");
        let want = module.closures[id.0 as usize].view();
        let label = want.name().unwrap_or("<anon>").to_string();
        if let Some(diff) = want.first_difference(&lir) {
            panic!("{label}: the payload's LIR differs from the frozen function at {diff}");
        }
    }
}

/// A code object with no LIR — an entry function, a hand-built one — answers
/// none, rather than an empty function the JIT would compile.
#[test]
fn a_payload_without_lir_answers_none() {
    let mut heap = FiberHeap::new();
    let t = CodeBuilder::new(vec![1, 2, 3], Arity::Exact(0), Vec::new()).build(&mut heap);
    assert!(t.lir().is_none(), "a hand-built code object grew LIR");
}

/// Freezing records the merge set and both release tables ascending, so the
/// frozen form a `JitTask` carries and the payload a view reads agree on order
/// as well as content (docs/impl/lir.md).
///
/// The counter-factual is a frozen function that keeps the lowerer's discovery
/// order: the payload sorts its copies for its binary searches, the two disagree,
/// and a test comparing them reads the order as a lost field.
#[test]
fn freezing_records_the_release_tables_ascending() {
    use crate::hir::region::StaticRegion;
    use crate::lir::{ConstRef, InstrRef, Reg, Terminator};

    let s = |n| StaticRegion::new(n).unwrap();
    let frozen = crate::lir::testkit::LirFixture::new(Arity::Exact(0))
        .head(|h| {
            h.merged_slots = vec![s(9), s(4), s(7)];
            h.frame_release_slots = vec![8, 3, 5];
            h.frame_release_regions = vec![s(13), s(11), s(12)];
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
    let view = frozen.view();
    assert_eq!(view.merged_slots(), &[s(4), s(7), s(9)]);
    assert_eq!(view.frame_release_slots(), &[3, 5, 8]);
    assert_eq!(view.frame_release_regions(), &[s(11), s(12), s(13)]);
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
    let mut rt = Runtime::without_stdlib();
    let v = run(&mut rt, LAMBDA);
    let closure = v.as_closure().expect("a closure").clone();
    let code = code_region(rt.heap(), &closure);
    let generation = rt.heap().region_generation(code.get());
    let copy = closure
        .template
        .lir()
        .expect("the payload carries LIR")
        .to_owned();
    let want = copy.clone();

    crate::value::arena::release_program_value(rt.heap(), v);
    assert_ne!(
        rt.heap().region_generation(code.get()),
        generation,
        "the closure was the code region's last holder, so the region must be freed \
         or this test frees nothing"
    );

    // Claim the freed pages again and write over them.
    let scribble = rt.heap().new_runtime_region();
    for _ in 0..64 {
        rt.heap()
            .alloc_region_slice_in_region(&[0xABu8; 4096], scribble);
    }

    if let Some(diff) = want.view().first_difference(&copy.view()) {
        panic!("the promotion copy changed when its payload was freed, at {diff}");
    }
}

/// A payload's LIR lands in its unit's code region and leaves with it. The
/// region holds at least the node records the function carries, and freeing
/// the last closure over a dropped unit returns the heap to the regions it
/// had before the compile.
///
/// The counter-factual is LIR kept beside the payload in Rust memory: the
/// region would not hold the bytes, and no region gauge could see them.
#[test]
fn a_payloads_lir_lands_in_its_code_region_and_leaves_with_it() {
    let mut rt = Runtime::without_stdlib();
    let baseline = rt.heap().active_region_count();
    let v = run(&mut rt, LAMBDA);
    let closure = v.as_closure().expect("a closure").clone();
    let code = code_region(rt.heap(), &closure);
    let lir = closure.template.lir().expect("the payload carries LIR");
    assert!(
        slice_bytes(rt.heap(), code) >= node_bytes(&lir),
        "the code region holds {} bytes of slices, fewer than the {} bytes of node \
         records the function carries",
        slice_bytes(rt.heap(), code),
        node_bytes(&lir)
    );

    crate::value::arena::release_program_value(rt.heap(), v);
    assert_eq!(
        rt.heap().active_region_count(),
        baseline,
        "freeing the last closure over a dropped unit left a region behind"
    );
}

/// The standard library's LIR is region pages the region gauges count: its
/// unit's code region holds every lambda's node records.
#[test]
fn the_stdlib_lir_lands_in_its_code_region() {
    let mut rt = Runtime::new();
    let (_module, unit) =
        frozen_and_emitted(&mut rt, crate::pipeline::sources::STDLIB, "stdlib.lisp");
    let code = RuntimeRegion::new(rt.heap().region_of_ptr(unit.entry().payload_backing()))
        .expect("the entry payload lives in a real region");
    let nodes: usize = lambdas(&unit).iter().map(|l| node_bytes(&l.view())).sum();
    assert!(
        slice_bytes(rt.heap(), code) >= nodes,
        "the stdlib's code region holds {} bytes of slices, fewer than the {nodes} \
         bytes of node records its lambdas carry, so the LIR is not in its pages",
        slice_bytes(rt.heap(), code)
    );
}
