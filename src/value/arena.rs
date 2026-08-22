//! Arena allocation layer.
//!
//! Every funnel here takes the `FiberHeap` it allocates on and names the
//! `RuntimeRegion` the value is born in, so a value's region and heap are visible
//! in its signature (the honesty invariant). Every region is mortal and
//! RC-reclaimable.
//!
//! Allocation paths:
//!
//! - `alloc()` / `alloc_region_slice()` — mint a fresh mortal region on `heap`
//!   and allocate into it (single-object test/builder scaffolding).
//! - `alloc_in_region()` (+ its slice twin) — allocate into a caller-resolved
//!   [`RuntimeRegion`] on `heap`.

use super::heap::HeapObject;
use super::Value;
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;

mod root;
pub use root::*;

// ── No-region allocation (fresh mortal region per call) ─────────────
//
// `alloc` mints a fresh mortal region on `heap` per object and allocates into it,
// for single-object `Value` builders that have no region to thread (test
// scaffolding). The value's owner governs the region's lifetime. Slice-backed
// builders (`string`/`array`/`bytes`/`set`) go through `value::build::*` instead,
// so their payload slice and header share one region (region/model.md).

/// Allocate a single heap object into a freshly-minted region on `heap`. For
/// objects with no `RegionSlice` payload (a slice would land in a different fresh
/// region — a UAF; slice-backed ctors use `value::build::*`).
pub fn alloc(heap: &mut FiberHeap, obj: HeapObject) -> Value {
    let region_id = heap.new_runtime_region();
    heap.alloc_in_region(obj, region_id)
}

/// Allocate a `RegionSlice` into a freshly-minted region on `heap`.
/// A convenience for test scaffolding (the external test crates build closure
/// env slices through it); production code resolves a region and uses
/// `alloc_region_slice_in_region`. The freshly-minted region is the slice's
/// own — callers that embed it in a header must build that header in the SAME
/// region (`alloc_region_slice_in_region` + `alloc_in_region`) when the value
/// will be reclaimed; the bare ctors that must co-locate go through
/// `value::build::*` instead.
pub fn alloc_region_slice<T: Copy + 'static>(
    heap: &mut FiberHeap,
    items: &[T],
) -> super::region_slice::RegionSlice<T> {
    let region_id = heap.new_runtime_region();
    heap.alloc_region_slice_in_region(items, region_id)
}

// ── Explicit-region allocation (compiler-resolved literal regions) ──
//
// A heap literal is an ordinary allocation born in its OWN solver-assigned
// region: `MaterializeConst` resolves that static slot to a physical
// `RuntimeRegion` per activation and materializes a fresh value into it. These
// take the region explicitly, like `MakeArrayMut`/`List`, so the literal is born
// in the right region (Rule 3).

/// Allocate a heap object into an explicit, caller-resolved mortal region.
pub fn alloc_in_region(heap: &mut FiberHeap, obj: HeapObject, region: RuntimeRegion) -> Value {
    heap.alloc_in_region(obj, region)
}

/// Allocate a `RegionSlice` into an explicit, caller-resolved mortal region.
pub fn alloc_region_slice_in_region<T: Copy + 'static>(
    heap: &mut FiberHeap,
    items: &[T],
    region: RuntimeRegion,
) -> super::region_slice::RegionSlice<T> {
    heap.alloc_region_slice_in_region(items, region)
}

// The root region and the process-root registry are `FiberHeap` fields
// (`root_region` / `process_roots`), so two embedded instances each govern their
// own. The accessors live on `FiberHeap`; `arena::root` operates through them.

/// No-op — heap values are reclaimed by region RC, not individually dropped.
/// # Safety
/// Always safe (no-op). Kept for API compatibility.
#[inline]
pub unsafe fn drop_heap(_value: Value) {}

// ── Runtime RC tracking ─────────────────────────────────────────────

/// Classify the region a heap value lives in. Returns `None` for non-heap
/// values (immediates) and for heap pointers not owned by any tracked region;
/// `Some(r)` for the mortal [`RuntimeRegion`] (id ≥ 2) the value lives in.
///
/// This is the funnel every runtime RC decision reads a value's region
/// through, so it carries the debug-build stale-deref generation check
/// (docs/impl/region/generations.md § "Region generations"): a value whose region was freed
/// panics here deterministically instead of yielding a recycled id.
pub fn region_of(heap: &FiberHeap, val: Value) -> Option<RuntimeRegion> {
    if !val.is_heap() {
        return None;
    }
    let ptr = val.as_heap_ptr()?;
    let rid = heap.region_of_ptr(ptr);
    RuntimeRegion::new(rid)
}

/// Region of a value for call-result retain/release, seeing through a
/// mutable-capture wrapper.
///
/// A mutable, captured binding (`@x`) holds its value inside a
/// `CaptureCell`, which lives in its own region with its own
/// `DecrefRegion` at the binding's scope end. The call-result a binding
/// is initialised from, however, is the value *inside* the cell — that
/// is the region the value-based `IncrefValueRegion`/`DecrefValueRegion`
/// must target. Targeting the cell's own region instead double-frees it
/// (the cell's `DecrefRegion` frees it a second time) and leaks the inner
/// value. Unwrap one level so retain/release reach the wrapped value.
pub fn result_region_of(heap: &FiberHeap, val: Value) -> Option<RuntimeRegion> {
    if val.is_capture_cell() {
        if let Some(cell) = val.as_capture_cell_raw() {
            return region_of(heap, *cell.borrow());
        }
    }
    region_of(heap, val)
}

/// Increment a region's reference count. `None` (a non-heap value's absent
/// region, i.e. an immediate) is a no-op. Every `RuntimeRegion` is mortal and
/// RC-tracked, so a present region always reaches the store.
pub fn incref_region(heap: &mut FiberHeap, id: Option<RuntimeRegion>) {
    let Some(r) = id else {
        return;
    };
    heap.incref_region(r);
}

/// The exhaustive set of Rule 5 escape sites — every place a runtime
/// reference-count is *raised* because a value escapes the scope it was born
/// in. This enum is the audit surface docs/impl/region/rules.md Rule 5 demands ("the
/// escape-site list being complete *is* correctness for the RC half"): every
/// runtime escape incref goes through [`incref_for_escape`], so `git grep
/// EscapeSite` enumerates the whole set in one greppable place. A missing arm
/// is a missing incref (a use-after-free); a spurious one is a leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeSite {
    /// A value stored into a mutable container (push/put/add/insert, or a
    /// same-cell rebind) — paired with `decref_removed_element` on removal.
    MutableStore,
    /// A capture cell's contents replaced (`UpdateCapture` / `StoreUpvalue`).
    CaptureStore,
    /// Immutable contents built at runtime point into another region (cons /
    /// list / array construction cross-region edge; the `IncrefRegion` opcode).
    ImmutableContents,
    /// A native pass-through result (`first`/`rest`/`get` …) that lives in a
    /// different region than the call allocated.
    NativeCallResult,
    /// A collection-as-function call-index result (`(arr i)` / `(m :k)`) that
    /// borrows a co-located or stored element from the collection's region
    /// rather than allocating fresh — the Rule-5 pass-through for the
    /// `call_collection` path, mirroring `NativeCallResult` for `get`/`first`.
    CollectionCallResult,
    /// A `(param)` call returns a value bound in the dynamic-binding frame.
    ParameterResolve,
    /// A heap value moved into a closure call as a non-captured argument. The
    /// caller hands the owned-param callee one owning reference (the callee
    /// releases it value-based at the param's `decref_point`). Raised only on
    /// the NON-tail closure path (`build_closure_env`); a tail call is a pure
    /// move (no incref) and the callee still releases. Natives never take this
    /// incref — they borrow args and the caller's own decref reclaims them.
    CallArgument,
    /// A function's tail result handed to its caller (`IncrefValueRegion` — the
    /// prediction-free return convention; the caller's `DecrefValueRegion`
    /// consumes it).
    ReturnValue,
    /// A message enqueued into a channel buffer (`chan/send`). The buffer is
    /// external to the region system — no free-time cascade balances it — so
    /// this retain IS the message's reference while it rides the buffer;
    /// `release_received_message` lowers it as the receive takes the message
    /// out (docs/impl/region/effects.md § `Sends`).
    ChanSend,
    /// A yielded / suspended value escapes into `fiber.signal`.
    SuspendEscape,
    /// An emitted value the scheduler holds via `fiber.signal` past the
    /// matching compiler-emitted `DecrefRegion`.
    EmitEscape,
    /// A child's parked payload re-installed as the propagating fiber's own
    /// `signal` (`fiber/propagate`). The install is a fresh park, so it owes the
    /// delivery reference its resumer's result release consumes — the child's
    /// park funded its own resumer, not this one (docs/impl/region/owner.md
    /// § "Park/unpark symmetry").
    PropagateEscape,
    /// A child fiber's set-once terminal result, park-retained until the fiber
    /// is freed (released by the signal scan — asymmetric by Rule 7).
    TerminalSignal,
    /// A heap value in a child fiber's inherited dynamic-parameter baseline,
    /// retained at the seed until the fiber is freed (released by the Fiber
    /// content scan's baseline walk — the terminal-signal shape;
    /// docs/impl/region/owner.md § "A child's inherited parameter baseline is
    /// a counted holder").
    ParamBaseline,
}

impl EscapeSite {
    fn as_str(self) -> &'static str {
        match self {
            EscapeSite::MutableStore => "mutable-store",
            EscapeSite::CaptureStore => "capture-store",
            EscapeSite::ImmutableContents => "immutable-contents",
            EscapeSite::NativeCallResult => "native-call-result",
            EscapeSite::CollectionCallResult => "collection-call-result",
            EscapeSite::ParameterResolve => "parameter-resolve",
            EscapeSite::CallArgument => "call-argument",
            EscapeSite::ReturnValue => "return-value",
            EscapeSite::ChanSend => "chan-send",
            EscapeSite::SuspendEscape => "suspend-escape",
            EscapeSite::EmitEscape => "emit-escape",
            EscapeSite::PropagateEscape => "propagate-escape",
            EscapeSite::TerminalSignal => "terminal-signal",
            EscapeSite::ParamBaseline => "param-baseline",
        }
    }
}

/// Raise `region`'s RC for a Rule 5 escape — the single funnel every runtime
/// escape incref flows through. Behaviourally identical to [`incref_region`];
/// `site` is the audit tag (it is what makes the escape-site set greppable) and
/// the `--trace=rc` label. `None` (a non-heap value) is a no-op.
#[inline]
pub fn incref_for_escape(heap: &mut FiberHeap, region: Option<RuntimeRegion>, site: EscapeSite) {
    if let Some(r) = region {
        if crate::config::get().has_trace("rc") {
            eprintln!("[trace:rc] incref_for_escape({r}) [{}]", site.as_str());
        }
    }
    incref_region(heap, region);
}

/// The "pass-through retain" (Rule 5, [`EscapeSite::NativeCallResult`]) shared
/// by every site that runs a native/intrinsic body into a freshly-minted
/// per-call region: `dispatch_native_call` and the conditionally-allocating
/// intrinsic opcode handlers (`%put`/`%del`/`%string-push`).
///
/// When the body's result lands in a region OTHER than the `minted` one — a
/// pass-through such as `first`/`get`, or an in-place `%put` on an `@struct`
/// that returns its argument — hand the caller exactly one owning reference to
/// the result's runtime region, so the caller's `DecrefValueRegion` balances
/// instead of freeing a region owned elsewhere. A *fresh* result (already in
/// `minted`) needs no retain: its allocation rc=1 is the handoff. An immediate
/// result has no region, so the incref no-ops.
#[inline]
pub fn pass_through_retain(heap: &mut FiberHeap, value: Value, minted: RuntimeRegion) {
    let result_region = region_of(heap, value);
    if result_region != Some(minted) {
        incref_for_escape(heap, result_region, EscapeSite::NativeCallResult);
    }
}

/// Decrement a region's reference count. `None` (an immediate) is a no-op
/// (symmetric with [`incref_region`]); every present `RuntimeRegion` reaches
/// the store.
pub fn decref_region(heap: &mut FiberHeap, id: Option<RuntimeRegion>) {
    let Some(r) = id else {
        return;
    };
    heap.decref_region(r);
}

/// Track a store into a mutable collection.
pub fn rebind_stored_element(heap: &mut FiberHeap, old: Value, new: Value) {
    let old_r = region_of(heap, old);
    let new_r = region_of(heap, new);
    if old_r == new_r {
        return;
    }
    incref_for_escape(heap, new_r, EscapeSite::MutableStore);
    decref_region(heap, old_r);
}

/// Track adding a value to a mutable collection.
pub fn incref_inserted_element(heap: &mut FiberHeap, val: Value) {
    let r = region_of(heap, val);
    if crate::config::get().has_trace("rc") && val.is_heap() {
        eprintln!(
            "[trace:rc] incref_inserted_element: val_type={} region={:?}",
            val.type_name(),
            r
        );
    }
    debug_assert!(
        !val.is_heap() || r.is_some(),
        "incref_inserted_element: heap value has no region — page header missing or corrupt"
    );
    incref_for_escape(heap, r, EscapeSite::MutableStore);
}

/// Track removing a value from a mutable collection.
pub fn decref_removed_element(heap: &mut FiberHeap, val: Value) {
    let r = region_of(heap, val);
    decref_region(heap, r);
}

mod mutate;
pub use mutate::*;

// ── Utility ─────────────────────────────────────────────────────────

/// Get a reference to a heap object from a Value.
///
/// # Safety
/// The Value must be a heap pointer (is_heap() returns true).
#[inline]
pub unsafe fn deref(value: Value) -> &'static HeapObject {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    let obj = &*ptr;
    // Tag/object agreement: the Value's tag bits must match the heap
    // object's discriminant. A mismatch is the canonical signature
    // of a use-after-free: the original allocation was freed (region
    // teardown, slab reuse) and the same address was repurposed for
    // a different HeapObject variant. The stale Value still has the
    // original tag but the memory now holds something else.
    //
    // If you hit this in debug, walk back to find what freed the region
    // while a Value still referenced it.
    // The first 8-byte block of the payload is dumped so a UAF panic
    // shows what's actually at the slot: all-zero distinguishes "the
    // region died and the page pool blanked its body" (docs/impl/
    // region/model.md § "Page recycling") from stale-data (slot reused
    // for a different HeapObject with its own discriminant bits in
    // place). Without it the variant
    // reported by `type_name()` is misleading — a zero-filled page
    // reads as whichever variant Rust's enum repr assigns to the
    // all-zero discriminant.
    // Compute the diagnostic only on mismatch — keep the happy path (every
    // deref in debug builds) free of the free-log lookup + allocation.
    #[cfg(debug_assertions)]
    if value.tag != obj.value_tag() {
        let attribution = crate::value::fiberheap::freelog::describe(value.payload as usize)
            .unwrap_or_else(|| "free-log empty (run with --trace=free)".to_string());
        panic!(
            "tag/object mismatch — use-after-free? value.tag=0x{:x} object={} \
             (variant's expected tag=0x{:x}) payload=0x{:x} first8=0x{:016x}; \
             {}\n    expansion context: {}; \
             see docs/regions.md and CONTRIBUTING.md on progressive constraint",
            value.tag,
            obj.type_name(),
            obj.value_tag(),
            value.payload,
            unsafe { *(value.payload as *const u64) },
            attribution,
            crate::value::fiberheap::freelog::context(),
        );
    }
    obj
}

// ── Test helpers ────────────────────────────────────────────────────
//
// Each takes the heap explicitly. A test with no VM in scope leaks one via
// [`leaked_test_heap`]; a test with a `VM`/`Runtime`/`TestHeap` passes that
// instance's heap.

/// Leak a fresh `FiberHeap` for a test that needs a raw heap pointer with no VM
/// in scope. Leaked deliberately, like [`crate::vm::VM::new`]'s private heap:
/// values built on it stay valid for the test process.
#[cfg(test)]
pub fn leaked_test_heap() -> *mut FiberHeap {
    Box::leak(Box::new(FiberHeap::new()))
}

/// Run `f`. A thin seam kept so test bodies read uniformly; test values are
/// built through an explicit heap (`TestHeap` / [`leaked_test_heap`]), so it
/// installs nothing.
#[cfg(test)]
pub fn with_test_region<R>(f: impl FnOnce() -> R) -> R {
    f()
}

/// Allocate a heap object into a fresh region on `heap`, returning (value, region).
#[cfg(test)]
pub fn alloc_in_fresh_region(
    heap: &mut FiberHeap,
    obj: super::heap::HeapObject,
) -> (Value, RuntimeRegion) {
    let rid = heap.new_runtime_region();
    let val = heap.alloc_in_region(obj, rid);
    (val, rid)
}

/// Get the RC of a region on `heap`.
#[cfg(test)]
pub fn region_rc(heap: &FiberHeap, id: RuntimeRegion) -> u32 {
    heap.region_rc(id)
}

/// Release one reference to a region on `heap`, freeing it at RC 0.
#[cfg(test)]
pub fn decref_if_present(heap: &mut FiberHeap, id: RuntimeRegion) {
    heap.decref_region_if_present(id);
}

#[cfg(test)]
mod tests;
