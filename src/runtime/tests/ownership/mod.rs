//! Runtime tests for the ownership forest, split by cut family. Shared growth
//! harness + discriminator live here; each submodule holds one family of pins.
use super::*;

mod anode;
mod fnode;
mod jit;
mod owner;
mod selfrec;
mod subtree;

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
