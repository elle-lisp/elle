//! Per-instance trait registry: default traitsets stamped at allocation.
//!
//! Each collection/sequence HeapTag has a default @struct traitset. The tables
//! are **instance state** living on the `FiberHeap` (tls.md: nothing
//! correctness-bearing is thread-centric), so two embedded instances on one
//! thread each carry their own and never cross-reference. The trait-method
//! natives themselves are static `&'static PrimitiveDef` immediates — process-
//! global and correctly shared, like every other primitive. Constructors read
//! the heap's registry entry and stamp it into the new object's `traits` field.

use std::collections::BTreeMap;

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapTag;
use crate::value::types::TableKey;
use crate::value::Value;

mod methods;
use methods::*;

/// Number of HeapTag variants (max index is CaptureCell = 28).
const NUM_TAGS: usize = 29;

/// Return the default traitset for a given HeapTag on `heap`. Returns
/// `Value::NIL` if `heap`'s tables are not built or the tag has no default.
#[inline]
pub fn default_traits_for(heap: &FiberHeap, tag: HeapTag) -> Value {
    heap.default_traits_for(tag)
}

/// Build `heap`'s default trait tables into its root region, idempotently.
/// Called from VM initialization before any collection allocation. The trait
/// structs are `alloc_root`'d into the heap's pinned root region (released by
/// the teardown sweep), and the per-tag table is stored on the heap itself.
pub fn init_default_traits(heap: &mut FiberHeap) {
    if heap.default_traits_built() {
        return; // already built for this instance
    }
    let mut table = vec![Value::NIL; NUM_TAGS];

    // Build method structs for :Sequence and :Collection protocols, then
    // assemble @struct traitsets for each collection type — all into this
    // heap's root region.
    let seq_methods = build_sequence_methods(heap);
    let coll_methods = build_collection_methods(heap);

    // Sequence + Collection types: array, @array, list, string, @string,
    // bytes, @bytes
    let seq_coll = make_traitset(heap, Some(seq_methods), Some(coll_methods));
    // Collection-only types: set, @set, struct, @struct
    let coll_only = make_traitset(heap, None, Some(coll_methods));

    table[HeapTag::LArray as usize] = seq_coll;
    table[HeapTag::LArrayMut as usize] = seq_coll;
    table[HeapTag::Pair as usize] = seq_coll;
    table[HeapTag::LString as usize] = seq_coll;
    table[HeapTag::LStringMut as usize] = seq_coll;
    table[HeapTag::LBytes as usize] = seq_coll;
    table[HeapTag::LBytesMut as usize] = seq_coll;
    table[HeapTag::LSet as usize] = coll_only;
    table[HeapTag::LSetMut as usize] = coll_only;
    table[HeapTag::LStruct as usize] = coll_only;
    table[HeapTag::LStructMut as usize] = coll_only;

    heap.set_default_traits(table);
}

/// Clear `heap`'s default-traits table during the teardown sweep. The trait
/// tables are `alloc_root`'d into the heap's root region, which the sweep
/// releases by RC; clearing the table drops the now-dangling `Value`s so a
/// post-teardown read returns `NIL` rather than a freed pointer. (With the
/// tables instance-owned there is no cross-instance cache to invalidate — the
/// next instance builds its own.)
pub fn reset_default_traits(heap: &mut FiberHeap) {
    heap.set_default_traits(Vec::new());
}

/// Read the traits field from a value.
///
/// Returns the traits @struct for heap objects, or `Value::NIL` for
/// immediates and infrastructure types. No fallback, no registry lookup.
pub fn get_traitset(val: &Value) -> Value {
    if !val.is_heap() {
        return Value::NIL;
    }
    unsafe { crate::value::heap::deref(*val).traits() }
}

/// Look up a trait method on a value and call it.
///
/// Reads the traits field (always populated for collection types),
/// looks up the protocol and method, and calls the method.
/// If the value's trait table doesn't contain the requested protocol,
/// falls back to the default traitset from the registry.
/// Returns `(SignalBits, Value)` directly.
pub fn dispatch_trait_method(
    val: &Value,
    protocol: &str,
    method: &str,
    args: &[Value],
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
) -> (SignalBits, Value) {
    let traits_val = get_traitset(val);
    if traits_val.is_nil() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: no trait table on {} value", method, val.type_name()),
            ),
        );
    }

    // Look up protocol in the value's trait table
    let mut protocol_val = lookup_keyword(&traits_val, protocol);

    // If protocol not found in the value's traits, try the default
    // traitset from the registry (user traits may override only some protocols)
    if protocol_val.is_nil() && val.is_heap() {
        let tag = unsafe { crate::value::heap::deref(*val) }.tag();
        let default = default_traits_for(ctx.heap_mut(), tag);
        if !default.is_nil() {
            protocol_val = lookup_keyword(&default, protocol);
        }
    }

    if protocol_val.is_nil() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: no :{} protocol on {} value",
                    method,
                    protocol,
                    val.type_name()
                ),
            ),
        );
    }

    let method_fn = lookup_keyword(&protocol_val, method);
    if method_fn.is_nil() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: no :{} method in :{} protocol",
                    method, method, protocol
                ),
            ),
        );
    }

    call_method_fn(&method_fn, protocol, method, args, ctx)
}

/// Call a resolved trait method (NativeFn or Closure). `ctx` is the calling
/// native's capability — its `alloc_region` is the outer call's result slot, so
/// the native-fn branch runs the method against it (a fresh result then lands in
/// the region `dispatch_native_call` reclaims); the closure branch re-enters the
/// driving VM through it.
fn call_method_fn(
    method_fn: &Value,
    protocol: &str,
    method: &str,
    args: &[Value],
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
) -> (SignalBits, Value) {
    // NativeFn — call directly, against the OUTER call's own `ctx`. A trait
    // method resolved here is the body of an outer native (`first`/`rest`/`nth`,
    // `length`, …) that already holds a compiler-assigned result slot; running
    // the method against that same `ctx` lands its fresh result in the outer
    // call's `alloc_region`, so `dispatch_native_call` recognises it as fresh and
    // the consumer's `DecrefValueRegion` reclaims it. Minting a SEPARATE
    // `boundary` region here instead stranded a genuinely-fresh result — the
    // tail-copy slice of `(rest [array])` — in a region distinct from
    // `alloc_region`, which `dispatch_native_call` then mis-read as a pass-through
    // and over-retained, leaking that region (pinned bounded by
    // `runtime::tests::ownership::region_native_trait_dispatch_fresh_result_reclaims`).
    // A borrowed-element method (`first`/`nth`) allocates nothing, so its result
    // still lives in the arg's region and is correctly pass-through-retained.
    if let Some(prim_fn) = method_fn.as_native_fn() {
        return prim_fn(ctx, args);
    }

    // Closure — call on the driving VM reached through the ctx. Passed as the
    // Value so the entry hands the body its executing-closure register (a
    // self-recursive trait method resolves its self-reference to it).
    if method_fn.as_closure().is_some() {
        match ctx.vm().call_closure(*method_fn, args) {
            Ok(v) => return (SIG_OK, v),
            Err(msg) => {
                return (SIG_ERROR, ctx.error("trait-error", msg));
            }
        }
    }

    // Not callable
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "{}:{}: trait method is not callable ({})",
                protocol,
                method,
                method_fn.type_name()
            ),
        ),
    )
}

/// Look up a keyword key in a struct without allocating a TableKey.
///
/// Trait tables are small (2–5 entries), so linear scan on the keyword
/// discriminant + string comparison avoids the String allocation that
/// `TableKey::Keyword(key.into())` would require on every dispatch.
fn lookup_keyword(val: &Value, key: &str) -> Value {
    // Immutable struct — linear scan (small tables)
    if let Some(entries) = val.as_struct() {
        for (k, v) in entries.iter() {
            if let TableKey::Keyword(ref s) = k {
                if s == key {
                    return *v;
                }
            }
        }
        return Value::NIL;
    }

    // Mutable struct — linear scan on values
    if let Some(map_ref) = val.as_struct_mut() {
        let borrowed = map_ref.borrow();
        for (k, v) in borrowed.iter() {
            if let TableKey::Keyword(ref s) = k {
                if s == key {
                    return *v;
                }
            }
        }
        return Value::NIL;
    }

    Value::NIL
}
