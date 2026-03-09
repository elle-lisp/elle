//! Heap arena and memory management primitives

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (arena/count) — return current heap arena object count
///
/// Returns a bare integer. Unlike arena/stats (which returns a struct),
/// this has zero measurement overhead — integers are immediate values.
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
    (
        SIG_QUERY,
        Value::cons(Value::keyword("arena/count"), Value::NIL),
    )
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
/// (destructors run by RegionExit). Only non-zero inside child fibers
/// (root fiber has no FiberHeap). Returns {:enters 0 :dtors-run 0} for root.
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

/// (environment) — return the current global environment as a struct
///
/// Returns a struct mapping keyword names to values for all defined
/// globals:
/// ```text
/// {:+ <native-fn> :cons <native-fn> :my-var 42 ...}
/// ```
pub(crate) fn prim_environment(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("environment: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("environment"), Value::NIL),
    )
}

/// (arena/set-object-limit n) or (arena/set-object-limit n :global)
///
/// Set max heap object count. Default scope is :fiber (child fiber's FiberHeap).
/// :global targets HEAP_ARENA (root fiber). On root fiber, :fiber is implicitly :global.
/// Pass nil as n to remove the limit.
/// Returns previous limit as int, or nil if previously unlimited.
///
/// Operates directly on thread-local state (no SIG_QUERY) to avoid allocating
/// cons cells for the query message — those allocations would themselves be
/// subject to the limit, creating a chicken-and-egg problem.
pub(crate) fn prim_arena_set_object_limit(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "arena/set-object-limit: expected 1-2 arguments, got {}",
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
    let is_global = args.len() == 2 && {
        if args[1].as_keyword_name() == Some("global") {
            true
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/set-object-limit: second argument must be :global".to_string(),
                ),
            );
        }
    };
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    let prev = if is_global || heap_ptr.is_null() {
        crate::value::heap::heap_arena_set_object_limit(limit)
    } else {
        unsafe { (*heap_ptr).set_object_limit(limit) }
    };
    let result = match prev {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/object-limit) or (arena/object-limit :global)
///
/// Get current object limit. Returns int or nil (unlimited).
///
/// Operates directly on thread-local state (no SIG_QUERY).
pub(crate) fn prim_arena_object_limit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "arena/object-limit: expected 0-1 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let is_global = args.len() == 1 && {
        if args[0].as_keyword_name() == Some("global") {
            true
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/object-limit: argument must be :global".to_string(),
                ),
            );
        }
    };
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    let limit = if is_global || heap_ptr.is_null() {
        crate::value::heap::heap_arena_object_limit()
    } else {
        unsafe { (*heap_ptr).object_limit() }
    };
    let result = match limit {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/bytes) or (arena/bytes :global)
///
/// Return bytes consumed. :fiber = bumpalo allocated_bytes() for child fibers
/// (0 for root). :global = HEAP_ARENA object count × 128 (estimated object size).
///
/// Operates directly on thread-local state (no SIG_QUERY).
pub(crate) fn prim_arena_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/bytes: expected 0-1 arguments, got {}", args.len()),
            ),
        );
    }
    let is_global = args.len() == 1 && {
        if args[0].as_keyword_name() == Some("global") {
            true
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/bytes: argument must be :global".to_string(),
                ),
            );
        }
    };
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    if is_global || heap_ptr.is_null() {
        // Global: estimate from object count × 128 bytes per object
        let bytes = crate::value::heap::heap_arena_len() * 128;
        (SIG_OK, Value::int(bytes as i64))
    } else {
        let bytes = unsafe { (*heap_ptr).allocated_bytes() };
        (SIG_OK, Value::int(bytes as i64))
    }
}

/// (arena/checkpoint) — return an opaque mark for the current root-fiber arena position.
///
/// Pass to arena/reset to reclaim all objects allocated after this point.
/// Dangerous: invalidates all Values allocated after the mark.
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
    (
        SIG_OK,
        Value::int(crate::value::heap::heap_arena_checkpoint() as i64),
    )
}

/// (arena/reset mark) — reclaim all root-fiber arena objects allocated after mark.
///
/// Runs Drop for freed objects. Dangerous: any Value pointing into the
/// freed region is now invalid.
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
    let mark = match args[0].as_int() {
        Some(n) if n >= 0 => n as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/reset: mark must be non-negative".to_string(),
                ),
            );
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("arena/reset: expected integer, got {}", args[0].type_name()),
                ),
            );
        }
    };
    let current = crate::value::heap::heap_arena_checkpoint();
    if mark > current {
        return (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "arena/reset: mark {} exceeds current arena count {}",
                    mark, current
                ),
            ),
        );
    }
    crate::value::heap::heap_arena_reset(mark);
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

/// (arena/peak) or (arena/peak :global) — return peak object count (high-water mark)
pub(crate) fn prim_arena_peak(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arena/peak: expected 0-1 arguments, got {}", args.len()),
            ),
        );
    }
    let is_global = args.len() == 1 && {
        if args[0].as_keyword_name() == Some("global") {
            true
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/peak: argument must be :global".to_string(),
                ),
            );
        }
    };
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    let peak = if is_global || heap_ptr.is_null() {
        crate::value::heap::heap_arena_peak()
    } else {
        unsafe { (*heap_ptr).peak_alloc_count() }
    };
    (SIG_OK, Value::int(peak as i64))
}

/// (arena/reset-peak) or (arena/reset-peak :global) — reset peak to current count, return previous peak
pub(crate) fn prim_arena_reset_peak(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "arena/reset-peak: expected 0-1 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let is_global = args.len() == 1 && {
        if args[0].as_keyword_name() == Some("global") {
            true
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    "arena/reset-peak: argument must be :global".to_string(),
                ),
            );
        }
    };
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    let prev = if is_global || heap_ptr.is_null() {
        crate::value::heap::heap_arena_reset_peak()
    } else {
        unsafe { (*heap_ptr).reset_peak() }
    };
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(0),
        doc: "Return current heap arena object count as an integer (zero measurement overhead).",
        params: &[],
        category: "meta",
        example: "(arena/count)",
        aliases: &["arena-count"],
    },
    PrimitiveDef {
        name: "arena/scope-stats",
        func: prim_scope_stats,
        effect: Effect::inert(),
        arity: Arity::Exact(0),
        doc: "Return scope allocation runtime stats as {:enters N :dtors-run N}. Only non-zero inside child fibers.",
        params: &[],
        category: "meta",
        example: "(arena/scope-stats)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/set-object-limit",
        func: prim_arena_set_object_limit,
        effect: Effect::inert(),
        arity: Arity::Range(1, 2),
        doc: "Set max heap object count. Pass nil to remove limit. Returns previous limit or nil.",
        params: &["n", "scope?"],
        category: "meta",
        example: "(arena/set-object-limit 10000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/object-limit",
        func: prim_arena_object_limit,
        effect: Effect::inert(),
        arity: Arity::Range(0, 1),
        doc: "Get current object limit. Returns int or nil (unlimited).",
        params: &["scope?"],
        category: "meta",
        example: "(arena/object-limit)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/bytes",
        func: prim_arena_bytes,
        effect: Effect::inert(),
        arity: Arity::Range(0, 1),
        doc: "Return bytes consumed. :fiber = bumpalo bytes, :global = estimated from object count × 128.",
        params: &["scope?"],
        category: "meta",
        example: "(arena/bytes)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/checkpoint",
        func: prim_arena_checkpoint,
        effect: Effect::inert(),
        arity: Arity::Exact(0),
        doc: "Return an opaque mark for the current root-fiber arena position. Pass to arena/reset to reclaim all objects allocated after this point. Dangerous: invalidates all Values allocated after the mark.",
        params: &[],
        category: "meta",
        example: "(arena/checkpoint)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/reset",
        func: prim_arena_reset,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Reclaim all root-fiber arena objects allocated after mark (from arena/checkpoint). Runs Drop for freed objects. Dangerous: any Value pointing into the freed region is now invalid.",
        params: &["mark"],
        category: "meta",
        example: "(let ((m (arena/checkpoint))) (cons 1 2) (arena/reset m))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/allocs",
        func: prim_arena_allocs,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Range(0, 1),
        doc: "Return peak object count (high-water mark). Optional :global scope.",
        params: &["scope?"],
        category: "meta",
        example: "(arena/peak)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/reset-peak",
        func: prim_arena_reset_peak,
        effect: Effect::inert(),
        arity: Arity::Range(0, 1),
        doc: "Reset peak to current count. Returns previous peak. Optional :global scope.",
        params: &["scope?"],
        category: "meta",
        example: "(arena/reset-peak)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arena/fiber-stats",
        func: prim_arena_fiber_stats,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Return heap stats for a suspended or dead fiber as a struct with :count, :bytes, :peak, :object-limit, :scope-enters, :dtors-run.",
        params: &["fiber"],
        category: "meta",
        example: "(let* ([f (fiber/new (fn [] 42) 1)] [_ (fiber/resume f nil)]) (arena/fiber-stats f))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "environment",
        func: prim_environment,
        effect: Effect::inert(),
        arity: Arity::Exact(0),
        doc: "Return the current global environment as a struct mapping keyword names to values.",
        params: &[],
        category: "meta",
        example: "(environment)",
        aliases: &[],
    },
];
