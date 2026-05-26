//! Heap arena and memory management primitives

use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (arena/count) — return current heap object count.
pub(crate) fn prim_arena_count(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let count = unsafe { (*heap_ptr).visible_len() };
    (SIG_OK, Value::int(count as i64))
}

/// (arena/stats) or (arena/stats fiber) — return heap arena statistics
pub(crate) fn prim_arena_stats(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        (
            SIG_QUERY,
            Value::pair(Value::keyword("arena/stats"), Value::NIL),
        )
    } else {
        (
            SIG_QUERY,
            Value::pair(Value::keyword("arena/stats"), args[0]),
        )
    }
}

/// (arena/set-object-limit n)
pub(crate) fn prim_arena_set_object_limit(args: &[Value]) -> (SignalBits, Value) {
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
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let prev = unsafe { (*heap_ptr).set_object_limit(limit) };
    let result = match prev {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/object-limit)
pub(crate) fn prim_arena_object_limit(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let limit = unsafe { (*heap_ptr).object_limit() };
    let result = match limit {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/bytes) — return bytes consumed by the current FiberHeap.
pub(crate) fn prim_arena_bytes(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let bytes = unsafe { (*heap_ptr).allocated_bytes() };
    (SIG_OK, Value::int(bytes as i64))
}

/// (arena/allocs thunk) — run thunk, return (result . net-allocs)
pub(crate) fn prim_arena_allocs(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_QUERY,
        Value::pair(Value::keyword("arena/allocs"), args[0]),
    )
}

/// (arena/peak) — return peak object count (high-water mark)
pub(crate) fn prim_arena_peak(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let peak = unsafe { (*heap_ptr).peak_alloc_count() };
    (SIG_OK, Value::int(peak as i64))
}

/// (arena/reset-peak) — reset peak to current count, return previous peak
pub(crate) fn prim_arena_reset_peak(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let prev = unsafe { (*heap_ptr).reset_peak() };
    (SIG_OK, Value::int(prev as i64))
}

/// (arena/region-of value) — return region ID for a heap value (0 for non-heap).
pub(crate) fn prim_arena_region_of(args: &[Value]) -> (SignalBits, Value) {
    let rid = crate::value::arena::region_of(args[0]);
    (SIG_OK, Value::int(rid as i64))
}

/// (arena/region-count) — return number of active regions.
pub(crate) fn prim_arena_region_count(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let count = unsafe { (*heap_ptr).active_region_count() };
    (SIG_OK, Value::int(count as i64))
}

/// (arena/region-info) — return array of {:id N :rc N :objects N} per region.
pub(crate) fn prim_arena_region_info(_args: &[Value]) -> (SignalBits, Value) {
    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
    debug_assert!(!heap_ptr.is_null(), "root heap must always be installed");
    let info = unsafe { (*heap_ptr).region_info_vec() };
    let items: Vec<Value> = info
        .into_iter()
        .map(|(id, rc, objects)| {
            use crate::value::heap::TableKey;
            use std::collections::BTreeMap;
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("id".to_string()), Value::int(id as i64));
            fields.insert(TableKey::Keyword("rc".to_string()), Value::int(rc as i64));
            fields.insert(
                TableKey::Keyword("objects".to_string()),
                Value::int(objects as i64),
            );
            Value::struct_from(fields)
        })
        .collect();
    (SIG_OK, Value::array(items))
}

primitive! {
    "debug/arena-stats" => prim_arena_stats {
        signal: (Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 }),
        arity: Arity::Range(0, 1),
        doc: "Return heap arena statistics.",
        params: &["fiber?"],
        category: "debug",
        example: "(debug/arena-stats)",
        aliases: &["arena/stats", "vm/arena", "arena-stats"],
    }
    "debug/arena-count" => prim_arena_count {
        signal: Signal::errors(),
        doc: "Return current heap object count.",
        category: "debug",
        example: "(debug/arena-count)",
        aliases: &["arena/count", "arena-count"],
    }
    "debug/arena-set-object-limit" => prim_arena_set_object_limit {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Set max heap object count. Pass nil to remove limit. Returns previous limit or nil.",
        params: &["n"],
        category: "debug",
        example: "(debug/arena-set-object-limit 10000)",
        aliases: &["arena/set-object-limit"],
    }
    "debug/arena-object-limit" => prim_arena_object_limit {
        signal: Signal::errors(),
        doc: "Get current object limit. Returns int or nil (unlimited).",
        category: "debug",
        example: "(debug/arena-object-limit)",
        aliases: &["arena/object-limit"],
    }
    "debug/arena-bytes" => prim_arena_bytes {
        signal: Signal::errors(),
        doc: "Return bytes consumed by the current FiberHeap.",
        category: "debug",
        example: "(debug/arena-bytes)",
        aliases: &["arena/bytes"],
    }
    "debug/arena-allocs" => prim_arena_allocs {
        signal: (Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 }),
        arity: Arity::Exact(1),
        doc: "Run thunk, return (result . net-allocs).",
        params: &["thunk"],
        category: "debug",
        example: "(debug/arena-allocs (fn [] (pair 1 2)))",
        aliases: &["arena/allocs"],
    }
    "debug/arena-peak" => prim_arena_peak {
        signal: Signal::errors(),
        doc: "Return peak object count (high-water mark).",
        category: "debug",
        example: "(debug/arena-peak)",
        aliases: &["arena/peak"],
    }
    "debug/arena-reset-peak" => prim_arena_reset_peak {
        signal: Signal::errors(),
        doc: "Reset peak to current count. Returns previous peak.",
        category: "debug",
        example: "(debug/arena-reset-peak)",
        aliases: &["arena/reset-peak"],
    }
    "debug/arena-region-of" => prim_arena_region_of {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return region ID for a heap value (0 for non-heap).",
        params: &["value"],
        category: "debug",
        example: "(debug/arena-region-of (pair 1 2))",
        aliases: &["arena/region-of"],
    }
    "debug/arena-region-count" => prim_arena_region_count {
        signal: Signal::errors(),
        doc: "Return number of active regions.",
        category: "debug",
        example: "(debug/arena-region-count)",
        aliases: &["arena/region-count"],
    }
    "debug/arena-region-info" => prim_arena_region_info {
        signal: Signal::errors(),
        doc: "Return array of {:id N :rc N :objects N} per active region.",
        category: "debug",
        example: "(debug/arena-region-info)",
        aliases: &["arena/region-info"],
    }
}
