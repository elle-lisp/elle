// audited: 2026-10-06
//! Which payload a header reads, and how long the code region behind it lives.
//! docs/impl/region/template.md
//!
//! The helpers here serve every themed submodule, so each one's `use super::*;`
//! resolves the same names.

pub(crate) use super::*;
use crate::hir::region::RuntimeRegion;
use crate::pipeline::compile_file;
use crate::runtime::Runtime;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::types::Arity;

mod lir;
mod payload;

/// The region the payload of `closure` lives in.
fn code_region(heap: &FiberHeap, closure: &Closure) -> RuntimeRegion {
    RuntimeRegion::new(heap.region_of_ptr(closure.template.payload_backing()))
        .expect("the payload lives in a real region")
}

/// Compile `src` as a file on `rt`, run it, and answer its value. The unit is
/// dropped before this returns, so whatever the value holds of its code is all
/// that holds it.
fn run(rt: &mut Runtime, src: &str) -> Value {
    let (vm, symbols, cctx) = rt.parts();
    let unit = compile_file(src, symbols, cctx, "<template>").expect("the source compiles");
    vm.execute(&unit).expect("the source runs")
}

/// The two closures a file returns as `[a b]`.
fn two_closures(v: Value) -> (Closure, Closure) {
    let items = v.as_array().expect("the file returns an array");
    (
        items[0].as_closure().expect("a closure").clone(),
        items[1].as_closure().expect("a closure").clone(),
    )
}

/// A file building one lambda twice: two creations of one code object.
const TWICE: &str = "(def mk (fn [] (fn [x] x))) [(mk) (mk)]";

/// One code object has one payload, however many headers name it. This is the
/// whole reason the payload is split out of the header: `MakeClosure` runs once
/// per closure *creation*, so a closure built in a loop would copy its
/// function's whole bytecode per iteration if the payload rode along.
///
/// The counter-factual is a payload copied per creation: the two headers would
/// then report different backing addresses for identical bytes.
#[test]
fn two_creations_of_one_lambda_share_one_payload() {
    let mut rt = Runtime::without_stdlib();
    let (a, b) = two_closures(run(&mut rt, TWICE));
    assert_ne!(
        a.template.value(),
        b.template.value(),
        "each creation allocates a header of its own"
    );
    assert_eq!(
        a.template.bytecode().as_ptr(),
        b.template.bytecode().as_ptr(),
        "the payload is shared; a second backing address means it was copied per \
         creation"
    );
    assert_eq!(
        a.template.constants().as_ptr(),
        b.template.constants().as_ptr(),
        "the constant pool is payload, shared with the bytecode"
    );
}

/// A header in another region takes a counted reference to the region its
/// payload lives in (Rule 5), and freeing the header gives it back.
///
/// The counter-factual is a payload backing that the alloc scan does not see:
/// the code region's count would not move with its headers, and the unit's
/// release would free it while a closure still named it.
#[test]
fn every_header_holds_a_counted_reference_on_its_code_region() {
    let mut rt = Runtime::without_stdlib();
    let pair = run(&mut rt, TWICE);
    let (a, _) = two_closures(pair);
    let code = code_region(rt.heap(), &a);
    let held = rt.heap().region_rc(code);
    crate::value::arena::release_program_value(rt.heap(), pair);
    let left = rt.heap().region_rc(code);
    assert_eq!(
        held - left,
        2,
        "freeing two closures must release the two references their headers held \
         ({held} before, {left} after)"
    );
}

/// A header can outlive the region a sibling header was born in. Freeing one
/// header's region must not take the shared payload with it — the surviving
/// header still reads its bytecode.
///
/// The trap this guards: the payload backing is a `RegionSlice` copied by value
/// into each header, so nothing about the header's own bytes says another
/// region owns the pages behind it.
#[test]
fn freeing_one_headers_region_leaves_a_siblings_payload_readable() {
    let mut heap = FiberHeap::new();
    let t = CodeBuilder::new(vec![11, 22, 33, 44], Arity::Exact(0), Vec::new()).build(&mut heap);

    let doomed = heap.new_runtime_region();
    let survivor = heap.new_runtime_region();
    let _ = heap.alloc_in_region(HeapObject::ClosureTemplate((*t).clone()), doomed);
    let live = heap.alloc_in_region(HeapObject::ClosureTemplate((*t).clone()), survivor);

    heap.decref_region(doomed);

    assert_eq!(
        TemplateRef::region(live).bytecode(),
        &[11, 22, 33, 44],
        "the shared payload must survive a sibling header's region"
    );
}

/// A compile unit owns one code region, and once the unit is dropped the
/// region lives exactly as long as the headers built from it: freeing the
/// last one frees the region. Nothing about a code object needs a second
/// reclamation mechanism.
///
/// The counter-factual is a code region some cache keeps a reference to: the
/// closure frees and its code stays resident until a sweep, or until teardown.
#[test]
fn a_dropped_units_code_region_frees_with_its_last_header() {
    let mut rt = Runtime::without_stdlib();
    let v = run(&mut rt, "(fn [x] x)");
    let code = code_region(rt.heap(), v.as_closure().expect("a closure"));
    let generation = rt.heap().region_generation(code.get());
    crate::value::arena::release_program_value(rt.heap(), v);
    assert_ne!(
        rt.heap().region_generation(code.get()),
        generation,
        "the unit is gone and so is the last closure over its code, so the code \
         region must be freed"
    );
}

/// The emitter fills a payload's child table with the headers its
/// `MakeClosure` instructions index, so a live closure's table is filled
/// exactly as a hydrated one's is.
///
/// The counter-factual is a table left empty on a live payload and answered
/// from somewhere else: the closure runs, and a dump or a `send` has to find
/// the children another way.
#[test]
fn a_live_closures_payload_child_table_is_filled() {
    let mut rt = Runtime::without_stdlib();
    let v = run(&mut rt, "(fn [] (fn [y] y))");
    let payload = v.as_closure().expect("a closure").template.payload();
    assert_eq!(
        payload.children().len(),
        1,
        "the outer lambda builds one lambda, so its child table holds one header"
    );
    let child = TemplateRef::region(payload.children()[0]);
    assert_eq!(child.arity(), Arity::Exact(1), "the child is `(fn [y] y)`");
}

/// A header is one payload slice and nothing else: no Rust-heap owner rides
/// on a code object.
///
/// The counter-factual is a blueprint pointer kept beside the slice "just for
/// the JIT", which passes every other test here and keeps a second copy of
/// every function alive.
#[test]
fn a_header_is_one_payload_slice() {
    assert_eq!(
        size_of::<ClosureTemplate>(),
        size_of::<crate::value::region_slice::RegionSlice<CodePayload>>(),
        "a header holds its payload slice and nothing else"
    );
    assert!(
        size_of::<HeapObject>() <= 128,
        "the closure template variant must not set the union's size, which it did \
         at 288 bytes; got {}",
        size_of::<HeapObject>()
    );
}

/// A macro expansion reclaims its scratch by balancing the references a scan
/// of heap contents cannot explain, and a unit's reference is held in Rust,
/// where that scan cannot reach it. So a unit compiled while a scope is open
/// must keep its code region through the reclaim.
///
/// The counter-factual is the reclaim taking the unit's reference: the code
/// region frees at the scope's end, and running the unit afterwards reads
/// recycled pages.
#[test]
fn a_unit_compiled_inside_a_macro_scope_survives_the_reclaim() {
    let mut rt = Runtime::without_stdlib();
    let scope = crate::value::arena::begin_macro_scope(rt.heap());
    let unit = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file("(fn [x] x)", symbols, cctx, "<template>").expect("the source compiles")
    };
    crate::value::arena::reclaim_macro_scope(rt.heap(), scope);
    let v = rt.vm().execute(&unit).expect("the unit still runs");
    let closure = v.as_closure().expect("a closure");
    assert_eq!(closure.template.arity(), Arity::Exact(1));
    assert!(
        !closure.template.bytecode().is_empty(),
        "the closure reads the code the unit compiled"
    );
}
