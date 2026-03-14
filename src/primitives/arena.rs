//! Heap arena and memory management primitives

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (arena/count)
///
/// Return current heap object count.
///
/// Operates directly on thread-local state (no SIG_QUERY).
pub(crate) fn prim_arena_count(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/count: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let count = unsafe { (*heap_ptr).len() };
    (SIG_OK, Value::int(count as i64))
}

/// (arena/stats) — return heap arena statistics
///
/// Returns a struct with :count (live objects) and :capacity (vec capacity).
pub(crate) fn prim_arena_stats(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/stats: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("arena/stats"), Value::NIL),
    )
}

/// (arena/scope-stats) — return scope allocation runtime statistics
///
/// Returns a struct with :enters (RegionEnter count) and :dtors-run
/// (destructors run by RegionExit). Returns live scope stats for the
/// current fiber (root or child).
pub(crate) fn prim_scope_stats(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "arena/scope-stats: expected 0 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("arena/scope-stats"), Value::NIL),
    )
}

/// (arena/set-object-limit n)
///
/// Set max heap object count on the current FiberHeap. Pass nil to remove
/// the limit. Returns previous limit as int, or nil if previously unlimited.
///
/// Operates directly on thread-local state (no SIG_QUERY) to avoid allocating
/// cons cells for the query message — those allocations would themselves be
/// subject to the limit, creating a chicken-and-egg problem.
pub(crate) fn prim_arena_set_object_limit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "arena/set-object-limit: expected 1 argument, got {}",
                    args.len()
                ),
            ),
        );
    }
    let limit = if args[0].is_nil() {
        None
    } else if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/set-object-limit: limit must be non-negative".to_string(),
                ),
            );
        }
        Some(n as usize)
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "arena/set-object-limit: expected integer or nil, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let prev = unsafe { (*heap_ptr).set_object_limit(limit) };
    let result = match prev {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/object-limit)
///
/// Get current object limit on the current FiberHeap. Returns int or nil (unlimited).
///
/// Operates directly on thread-local state (no SIG_QUERY).
pub(crate) fn prim_arena_object_limit(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "arena/object-limit: expected 0 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let limit = unsafe { (*heap_ptr).object_limit() };
    let result = match limit {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/bytes)
///
/// Return bytes consumed by the current FiberHeap.
///
/// Operates directly on thread-local state (no SIG_QUERY).
pub(crate) fn prim_arena_bytes(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/bytes: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let bytes = unsafe { (*heap_ptr).allocated_bytes() };
    (SIG_OK, Value::int(bytes as i64))
}

/// (arena/checkpoint) — return an opaque mark for the current arena position.
///
/// Returns an opaque External value wrapping an ArenaMark. Pass only to
/// arena/reset. Do not inspect or store as an integer.
///
/// Dangerous: any Value allocated after this mark becomes invalid after
/// arena/reset with this mark.
pub(crate) fn prim_arena_checkpoint(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/checkpoint: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    let mark = crate::value::heap::heap_arena_mark();
    // Wrap in External so the mark survives across VM state without
    // being mistaken for an integer.
    (SIG_OK, Value::external("arena/checkpoint", mark))
}

/// (arena/reset mark) — reclaim arena objects allocated after mark.
///
/// Runs destructors for freed objects. Bump memory is not reclaimed
/// (bumpalo does not support position-based deallocation without scope
/// marks). Objects are logically freed: destructors run, alloc_count
/// decremented.
///
/// Dangerous: any Value pointing into the freed region is now invalid.
pub(crate) fn prim_arena_reset(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/reset: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Extract the ArenaMark from the External wrapper.
    let mark: &crate::value::ArenaMark = match args[0].as_external::<crate::value::ArenaMark>() {
        Some(m) => m,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "arena/reset: expected an arena/checkpoint value, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    // Validate that the mark is not in the future.
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    if !heap_ptr.is_null() {
        let current_count = unsafe { (*heap_ptr).len() };
        if mark.position() > current_count {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!(
                        "arena/reset: mark position {} exceeds current count {}",
                        mark.position(),
                        current_count
                    ),
                ),
            );
        }
        // Clone the mark because release() takes ownership.
        let m = crate::value::ArenaMark::new_full(
            mark.position(),
            mark.dtor_len(),
            mark.custom_ptrs_len(),
            mark.bump_depth(),
        );
        unsafe { (*heap_ptr).release(m) };
    }
    (SIG_OK, Value::NIL)
}

/// (arena/allocs thunk) — run thunk, return (result . net-allocs)
///
/// Sends SIG_QUERY with (:arena/allocs . thunk). The VM handles this
/// specially: it snapshots the heap count, calls the thunk, snapshots
/// again, and returns a cons of (result . net-allocs).
pub(crate) fn prim_arena_allocs(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/allocs: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("arena/allocs"), args[0]),
    )
}

/// (arena/peak) — return peak object count (high-water mark)
pub(crate) fn prim_arena_peak(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/peak: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let peak = unsafe { (*heap_ptr).peak_alloc_count() };
    (SIG_OK, Value::int(peak as i64))
}

/// (arena/reset-peak) — reset peak to current count, return previous peak
pub(crate) fn prim_arena_reset_peak(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/reset-peak: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let prev = unsafe { (*heap_ptr).reset_peak() };
    (SIG_OK, Value::int(prev as i64))
}

/// (arena/fiber-stats fiber) — return heap stats for a suspended or dead fiber
pub(crate) fn prim_arena_fiber_stats(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/fiber-stats: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("arena/fiber-stats"), args[0]),
    )
}

/// Declarative primitive definitions for arena operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "arena/stats",
        func: prim_arena_stats,
        signal: Signal { bits: SignalBits::new(SIG_QUERY.0 | SIG_ERROR.0), propagates: 0 },
        arity: Arity::Exact(0),
        doc: "Return heap arena statistics as a struct with :count and :capacity.",
        params: &[],
        category: "meta",
        example: "(arena/stats)",
        aliases: &["vm/arena", "arena-stats"],
    },
    PrimitiveDef {
        name: "arena/count",
        func: prim_arena_count,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Return current heap object count.",
        params: &[],
        category: "meta",
        example: "(arena/count)",
        aliases: &["arena-count"],
    },
    PrimitiveDef {
        name: "arena/scope-stats",
        func: prim_scope_stats,
        signal: Signal { bits: SignalBits::new(SIG_QUERY.0 | SIG_ERROR.0), propagates: 0 },
        arity: Arity::Exact(0),
        doc: "Return scope allocation runtime stats as {:enters N :dtors-run N}. Returns live scope stats for the current fiber.",
        params: &[],
        category: "meta",
        example: "(arena/scope-stats)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/set-object-limit",
        func: prim_arena_set_object_limit,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Set max heap object count. Pass nil to remove limit. Returns previous limit or nil.",
        params: &["n"],
        category: "meta",
        example: "(arena/set-object-limit 10000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/object-limit",
        func: prim_arena_object_limit,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Get current object limit. Returns int or nil (unlimited).",
        params: &[],
        category: "meta",
        example: "(arena/object-limit)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/bytes",
        func: prim_arena_bytes,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Return bytes consumed by the current FiberHeap.",
        params: &[],
        category: "meta",
        example: "(arena/bytes)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/checkpoint",
        func: prim_arena_checkpoint,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Return an opaque checkpoint for the current arena position. Pass to arena/reset only. The return value is an opaque external — do not treat it as an integer. Dangerous: invalidates all Values allocated after the mark.",
        params: &[],
        category: "meta",
        example: "(let ((m (arena/checkpoint))) (cons 1 2) (arena/reset m))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/reset",
        func: prim_arena_reset,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Reclaim arena objects allocated after checkpoint mark. Runs destructors for freed objects. Bump memory is not reclaimed. Dangerous: any Value pointing into the freed region is now invalid.",
        params: &["mark"],
        category: "meta",
        example: "(let ((m (arena/checkpoint))) (cons 1 2) (arena/reset m))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/allocs",
        func: prim_arena_allocs,
        signal: Signal { bits: SignalBits::new(SIG_QUERY.0 | SIG_ERROR.0), propagates: 0 },
        arity: Arity::Exact(1),
        doc: "Run thunk, return (result . net-allocs) where net-allocs is the net heap objects allocated.",
        params: &["thunk"],
        category: "meta",
        example: "(arena/allocs (fn [] (cons 1 2)))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/peak",
        func: prim_arena_peak,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Return peak object count (high-water mark).",
        params: &[],
        category: "meta",
        example: "(arena/peak)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/reset-peak",
        func: prim_arena_reset_peak,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Reset peak to current count. Returns previous peak.",
        params: &[],
        category: "meta",
        example: "(arena/reset-peak)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/fiber-stats",
        func: prim_arena_fiber_stats,
        signal: Signal { bits: SignalBits::new(SIG_QUERY.0 | SIG_ERROR.0), propagates: 0 },
        arity: Arity::Exact(1),
        doc: "Return heap stats for a suspended or dead fiber as a struct with :count, :bytes, :peak, :object-limit, :scope-enters, :dtors-run.",
        params: &["fiber"],
        category: "meta",
        example: "(let* ([f (fiber/new (fn [] 42) 1)] [_ (fiber/resume f nil)]) (arena/fiber-stats f))",
        aliases: &[],
    },

];
