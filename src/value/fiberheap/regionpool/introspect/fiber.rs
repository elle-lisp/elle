// audited: 2026-10-20
//! The cross-region references a fiber object holds: its closure, its result, its closure value and its parameter baseline.
//!
//! docs/impl/region/model.md
//! docs/impl/region/owner.md

use crate::value::{FiberHandle, Value};

/// Route each cross-region reference the fiber behind `handle` holds through
/// `check`. The fiber's `traits` edge is not among them: the caller tracks it
/// with every other traitable variant.
pub(super) fn fiber_edges(handle: &FiberHandle, check: &mut impl FnMut(&Value)) {
    // Keep the fiber's (immutable) closure alive. The
    // closure's env RegionSlice backing and its captured Values
    // live in the region where the closure was built — usually a
    // *different* activation than the fiber. Without a cross-region
    // edge here, the spawning activation frees that region while
    // the parked fiber still references it (closure env reads as
    // garbage on first resume).
    let mut env_vals: Vec<Value> = Vec::new();
    let mut backing = Value::NIL;
    // A parked/dead fiber holds its result/yield value in `signal`,
    // read later by `fiber/value`. That value lives in whatever
    // region the fiber allocated it in — a region the parent will
    // `DecrefValueRegion` when the producing call's decref_point fires.
    // Track it as a cross-region edge so the fiber keeps it alive
    // until the fiber itself dies (cascade-decref here at free).
    // The matching incref is added when the fiber parks/dies (see
    // `incref_signal_region` in src/vm/fiber/refcount.rs).
    let mut signal_val = Value::NIL;
    // The fiber's closure holds a template (a region-allocated
    // `HeapObject::ClosureTemplate`) usually CO-region with the env
    // backing, but for an EMPTY-env closure there is no backing synth —
    // so the template edge must be tracked explicitly, else the fiber's
    // own template region is freed while the parked fiber still needs it
    // (`closure.template.code()`/arity read garbage on first resume).
    let mut template_val = Value::NIL;
    // The fiber's `closure_value` — the wrapper VALUE it installs as the
    // body's executing-closure register (`Fiber::closure_value`,
    // `pending_entry_closure` on first resume). For a `MakeClosure`
    // closure its region coincides with the env/template region tracked
    // above (a duplicate edge, balanced against a duplicate free-cascade
    // decref by the counted `outgoing` multiset). But a `squelch`/`attune`
    // wrapper (src/primitives/meta/syntaxops.rs) shares the SOURCE
    // closure's template and env yet is itself a fresh closure value in a
    // DIFFERENT region — so its region is reachable ONLY through
    // `closure_value`. Without this edge that region frees at its binding's
    // decref_point while the fiber still holds the value, and a later
    // free's cross-ref scan reads the freed page
    // (tests/impl/region-squelch-fiber-uaf.lisp).
    let mut closure_value = Value::NIL;
    // The seeded parameter BASELINE (frames[0], exactly when
    // `param_baseline_seeded`) was retained at the seed with a
    // recorded fiber → value edge per heap entry
    // (`retain_param_baseline`); walking it here is the symmetric
    // release at the fiber object's free. The fiber's own later
    // `parameterize` frames are NOT walked — their values belong
    // to the parked activation (docs/impl/region/owner.md).
    let mut param_vals: Vec<Value> = Vec::new();
    let _ = handle.try_with(|fib| {
        closure_value = fib.closure_value;
        if fib.param_baseline_seeded {
            if let Some(baseline) = fib.param_frames.first() {
                for &(_, v) in baseline {
                    param_vals.push(v);
                }
            }
        }
        // An empty env uses a dangling sentinel pointer (not in any
        // region page) — skip it. For a non-empty env, synthesize a
        // heap Value at the backing so the shared `check` routes the
        // closure's own region (where the RegionSlice lives) the
        // same way as any cross-region ref.
        if !fib.closure.env.is_empty() {
            let p = fib.closure.env.as_ptr() as *const ();
            backing = Value::from_heap_ptr(p, crate::value::repr::TAG_ARRAY);
        }
        template_val = fib.closure.template.value();
        for v in fib.closure.env.iter() {
            env_vals.push(*v);
        }
        if let Some((bits, v)) = fib.signal {
            // Only terminal results are park-retained (see
            // src/vm/fiber/refcount.rs `is_terminal_signal`); match that here so
            // the cascade-decref balances the park-retain. Yield /
            // suspending signal values are not pinned.
            if crate::vm::fiber::is_terminal_signal(bits) {
                signal_val = v;
            }
        }
    });
    if backing != Value::NIL {
        check(&backing);
    }
    if template_val != Value::NIL {
        check(&template_val);
    }
    for v in &env_vals {
        check(v);
    }
    check(&signal_val);
    check(&closure_value);
    for v in &param_vals {
        check(v);
    }
}
