//! Runtime tests for the ownership forest, split by cut family. Shared growth
//! harness + discriminator live here; each submodule holds one family of pins.
use super::*;

mod anode;
mod fnode;
mod frame;
mod jit;
mod native;
mod owner;
mod selfrec;
mod subtree;

/// A closure over hand-emitted bytecode, for driving a fiber body no production
/// lowering can build yet. The zero-arity template wraps the bytecode +
/// constants exactly as a compiled thunk would.
pub(super) fn fiber_body_closure(
    bc: crate::compiler::bytecode::Bytecode,
) -> std::rc::Rc<crate::value::Closure> {
    use std::rc::Rc;
    Rc::new(crate::value::Closure {
        template: crate::value::TemplateRef::new(Rc::new(crate::value::ClosureTemplate::new(
            Rc::new(bc.instructions),
            crate::value::Arity::Exact(0),
            Rc::new(bc.constants),
        ))),
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: crate::value::SignalBits::EMPTY,
    })
}

/// A child fiber over `closure`, plus its heap value (built through an `Alloc`
/// ctx into a region of its own, which the caller releases per cycle).
pub(super) fn child_fiber(
    heap: &mut crate::value::fiberheap::FiberHeap,
    closure: std::rc::Rc<crate::value::Closure>,
) -> (crate::value::FiberHandle, crate::value::Value) {
    let handle = crate::value::FiberHandle::new(crate::value::Fiber::new(
        closure,
        crate::value::SignalBits::EMPTY,
    ));
    let ctx = crate::primitives::ctx::Alloc::new(heap);
    let fiber_value = ctx.fiber_from_handle(handle.clone());
    (handle, fiber_value)
}

/// Compile `src` once (isolating runtime region behaviour from compile scratch),
/// warm up, then run the same bytecode 50 times, returning the net live-region
/// count delta. The shared steady-state harness every discarded-shape reclamation
/// pin below uses — the ownership forest is unconditional, so there is no flag to
/// vary; a shape reads bounded iff the forest reclaims it. `without_stdlib` keeps
/// the measurement to region count; the trustworthy UAF oracle is full-stdlib
/// `--trace=guardfree` (the elle corpus).
pub(super) fn steady_region_growth(src: &str) -> i64 {
    use crate::pipeline::compile_file_repl;
    let mut rt = Runtime::without_stdlib();
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil(), "the discarded-shape program returns nil");
    }
    let baseline = rt.heap().active_region_count() as i64;
    for _ in 0..50 {
        let (vm, symbols, cctx) = rt.parts();
        let v = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs");
        assert!(v.is_nil());
    }
    rt.heap().active_region_count() as i64 - baseline
}

/// Growth of `gauge` across 200 executions of `body`, sampled **mid-run by the
/// program** — after 50 warm-up iterations and again after 250 — and returned as
/// the raw delta the program computes.
///
/// The mid-run sampling is what makes this a per-call gauge rather than a
/// per-program one: a shape whose release is hoisted out of the driving loop
/// fires once and reads 0 either way, while a genuine per-call strand shows as
/// ~200. `rt` chooses the stdlib (a full runtime churns regions of its own; an
/// isolated one keeps the reading to the shape under test), and `gauge` chooses
/// the heap dimension — `arena/region-count` for whole regions,
/// `arena/count` for objects, since a shape can strand a region without growing
/// the object count and vice versa.
///
/// Every reading is void without a live-growth discriminator beside it: pair the
/// subject with a shape that legitimately retains (the self-referential
/// accumulator `(assign acc (%pair n acc))`) and assert that one grows.
///
/// The gauge natives are opaque to inference, so the closing subtraction is a
/// `%sub` under a `match` dispatch that proves both samples `:integer` — which
/// also keeps the harness working on a runtime with no stdlib `-`.
pub(super) fn mid_run_growth(mut rt: Runtime, prelude: &str, body: &str, gauge: &str) -> i64 {
    use crate::pipeline::compile_file_repl;
    let src = format!(
        "{prelude} (var n 0) \
         (while (%lt n 50) {body} (assign n (%add n 1))) \
         (def c50 ({gauge})) \
         (while (%lt n 250) {body} (assign n (%add n 1))) \
         (def c250 ({gauge})) \
         (match (type-of c250) :integer \
           (match (type-of c50) :integer (%sub c250 c50) _ -1) _ -1)"
    );
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(&src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, symbols, cctx) = rt.parts();
    vm.execute_scheduled(&result.bytecode, symbols, cctx)
        .expect("runs")
        .as_int()
        .expect("program returns the gauge delta as an int")
}

/// The live-growth discriminator for [`mid_run_growth`]: a self-referential
/// accumulator retains every prior by reference, so both heap dimensions must
/// grow ~1 per iteration. A subject reading near zero is real reclamation only
/// beside a large reading here.
pub(super) fn mid_run_discriminator(rt: Runtime, gauge: &str) -> i64 {
    mid_run_growth(rt, "(def @acc nil)", "(assign acc (%pair n acc))", gauge)
}

/// The built-in live-growth discriminator now that the ownership forest is
/// unconditional and every reclaimable cycle below reads bounded: a single mutable
/// `@array` that holds ITSELF (`a ⊇ a`). This is the degenerate mutable self-cycle —
/// per-region RC cannot collect it (the self-reference keeps the count at 1), and the
/// forest's cycle cuts do not reclaim it: MERGE needs static-slot members, the
/// co-owned group free needs a ≥2-member mutual cycle, and no container holds it. So
/// re-run in the discarded top-level harness it leaks exactly one region per run. A
/// near-zero SUBJECT growth is real reclamation ONLY beside a positive discriminator
/// growth (else the gauge is dead and every "bounded" reading void). It is a deliberate
/// uncollectable shape — the single-region face of the class-8 boundary
/// (docs/impl memory model § "Closing every residual"); if a future cut reclaims a
/// self-referential mutable region, this discriminator (and the gauge-live preconditions
/// that read it) go red, forcing a re-choice.
pub(super) const LEAK_DISCRIMINATOR: &str = "(begin (let [a (@array)] (%array-push a a) nil) nil)";

/// Per-run live-region growth of [`LEAK_DISCRIMINATOR`] over the shared harness.
/// Positive by construction — the gauge-live precondition for the bounded subject
/// assertions.
pub(super) fn leak_discriminator() -> i64 {
    steady_region_growth(LEAK_DISCRIMINATOR)
}
