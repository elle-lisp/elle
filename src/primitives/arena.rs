//! Heap arena and memory management primitives

use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::Value;

/// (arena/count) — return current heap object count.
pub(crate) fn prim_arena_count(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let count = ctx.heap_mut().visible_len();
    (SIG_OK, Value::int(count as i64))
}

/// (arena/stats) or (arena/stats fiber) — return heap arena statistics
pub(crate) fn prim_arena_stats(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() {
        (
            SIG_QUERY,
            ctx.pair(Value::keyword("arena/stats"), Value::NIL),
        )
    } else {
        (SIG_QUERY, ctx.pair(Value::keyword("arena/stats"), args[0]))
    }
}

/// (arena/set-object-limit n)
pub(crate) fn prim_arena_set_object_limit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let limit = if args[0].is_nil() {
        None
    } else if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    "arena/set-object-limit: limit must be non-negative".to_string(),
                ),
            );
        }
        Some(n as usize)
    } else {
        return type_error!(ctx, args[0], "arena/set-object-limit", "integer or nil");
    };
    let prev = ctx.heap_mut().set_object_limit(limit);
    let result = match prev {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/object-limit)
pub(crate) fn prim_arena_object_limit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let limit = ctx.heap_mut().object_limit();
    let result = match limit {
        Some(n) => Value::int(n as i64),
        None => Value::NIL,
    };
    (SIG_OK, result)
}

/// (arena/bytes) — return bytes consumed by the current FiberHeap.
pub(crate) fn prim_arena_bytes(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let bytes = ctx.heap_mut().allocated_bytes();
    (SIG_OK, Value::int(bytes as i64))
}

/// (arena/page-claims) — pages claimed from the heap's page pool, monotonic.
///
/// The page dimension the object and region gauges do not show: regions never
/// share pages, so a shape can hold its object count flat and still claim a
/// page per call. A delta across a fixed window is that shape's page cost
/// (docs/impl/region/model.md § "Page recycling").
pub(crate) fn prim_arena_page_claims(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let claims = ctx.heap_mut().page_claims();
    (SIG_OK, Value::int(claims as i64))
}

/// (arena/allocs thunk) — run thunk, return (result . net-allocs)
pub(crate) fn prim_arena_allocs(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_QUERY, ctx.pair(Value::keyword("arena/allocs"), args[0]))
}

/// (arena/peak) — return peak object count (high-water mark)
pub(crate) fn prim_arena_peak(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let peak = ctx.heap_mut().peak_alloc_count();
    (SIG_OK, Value::int(peak as i64))
}

/// (arena/total-allocs) — cumulative objects ever minted (monotonic).
pub(crate) fn prim_arena_total_allocs(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let total = ctx.heap_mut().total_alloc_count();
    (SIG_OK, Value::int(total as i64))
}

/// (arena/reset-peak) — reset peak to current count, return previous peak
pub(crate) fn prim_arena_reset_peak(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let prev = ctx.heap_mut().reset_peak();
    (SIG_OK, Value::int(prev as i64))
}

/// (arena/region-of value) — return region ID for a heap value (0 for non-heap).
pub(crate) fn prim_arena_region_of(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let rid = crate::value::arena::region_of(ctx.heap_mut(), args[0]).map_or(0, |r| r.get());
    (SIG_OK, Value::int(rid as i64))
}

/// (arena/dump) — print every live mortal region (id, RC, object count, and the
/// object tags it holds) to stderr. The leak-localising companion to
/// `arena/count` / `arena/region-info`: when a count says memory grew, the per-
/// region object *tags* name the unfreed value (a stray `Fiber` / `Closure`
/// region). Returns nil.
pub(crate) fn prim_arena_dump(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    eprintln!("--- region dump ---");
    ctx.heap_mut().debug_dump();
    (SIG_OK, Value::NIL)
}

/// (arena/region-count) — return number of active regions.
pub(crate) fn prim_arena_region_count(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let count = ctx.heap_mut().active_region_count();
    (SIG_OK, Value::int(count as i64))
}

/// (arena/region-ids) — physical region ids issued, one past the largest id ever
/// minted from scratch.
///
/// The *id* dimension the object, byte, and page gauges cannot show: a minted id
/// that never allocates holds no object, no page, and no reference count, yet
/// never returns to the free list (docs/impl/region/model.md § "Physical id
/// recycling"). A mint that recycles leaves this alone, so a delta across a fixed
/// window of a steady-state loop must be zero.
pub(crate) fn prim_arena_region_ids(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let issued = ctx.heap_mut().region_ids_issued();
    (SIG_OK, Value::int(i64::from(issued)))
}

/// (arena/region-table) — entries in the region table, one past the largest
/// physical region id ever made live. What the table costs resident, in slots;
/// `arena/region-ids` is the gauge that detects an id leak.
pub(crate) fn prim_arena_region_table(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let len = ctx.heap_mut().region_table_len();
    (SIG_OK, Value::int(len as i64))
}

/// (arena/region-info) — return array of {:id N :rc N :objects N} per region.
pub(crate) fn prim_arena_region_info(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let info = ctx.heap_mut().region_info_vec();
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
            ctx.struct_from(fields)
        })
        .collect();
    (SIG_OK, ctx.array(items))
}

primitive! {
    "debug/arena-stats" => prim_arena_stats {
        signal: Signal::query_errors(),
        arity: Arity::Range(0, 1),
        doc: "Return heap arena statistics.",
        params: &["fiber?"],
        category: "debug",
        example: "(debug/arena-stats)",
        aliases: &["arena/stats", "vm/arena", "arena-stats"],
        effect: RegionEffect::Fresh,
    }
    "debug/arena-count" => prim_arena_count {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return current heap object count.",
        category: "debug",
        example: "(debug/arena-count)",
        aliases: &["arena/count", "arena-count"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-set-object-limit" => prim_arena_set_object_limit {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Set max heap object count. Pass nil to remove limit. Returns previous limit or nil.",
        params: &["n"],
        category: "debug",
        example: "(debug/arena-set-object-limit 10000)",
        aliases: &["arena/set-object-limit"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-object-limit" => prim_arena_object_limit {
        signal: Signal::errors(),
        doc: "Get current object limit. Returns int or nil (unlimited).",
        category: "debug",
        example: "(debug/arena-object-limit)",
        aliases: &["arena/object-limit"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-bytes" => prim_arena_bytes {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return bytes consumed by the current FiberHeap.",
        category: "debug",
        example: "(debug/arena-bytes)",
        aliases: &["arena/bytes"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-page-claims" => prim_arena_page_claims {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return pages claimed from the heap's page pool (monotonic, never decremented on release).",
        category: "debug",
        example: "(debug/arena-page-claims)",
        aliases: &["arena/page-claims"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-allocs" => prim_arena_allocs {
        signal: Signal::query_errors(),
        arity: Arity::Exact(1),
        doc: "Run thunk, return (result . net-allocs).",
        params: &["thunk"],
        category: "debug",
        example: "(debug/arena-allocs (fn [] (pair 1 2)))",
        aliases: &["arena/allocs"],
        effect: RegionEffect::Fresh,
    }
    "debug/arena-peak" => prim_arena_peak {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return peak object count (high-water mark).",
        category: "debug",
        example: "(debug/arena-peak)",
        aliases: &["arena/peak"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-total-allocs" => prim_arena_total_allocs {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return cumulative objects ever minted (monotonic, never decremented on free).",
        category: "debug",
        example: "(debug/arena-total-allocs)",
        aliases: &["arena/total-allocs"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-reset-peak" => prim_arena_reset_peak {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Reset peak to current count. Returns previous peak.",
        category: "debug",
        example: "(debug/arena-reset-peak)",
        aliases: &["arena/reset-peak"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-region-of" => prim_arena_region_of {
        ret: RetType::Int,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return region ID for a heap value (0 for non-heap).",
        params: &["value"],
        category: "debug",
        example: "(debug/arena-region-of (pair 1 2))",
        aliases: &["arena/region-of"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-dump" => prim_arena_dump {
        signal: Signal::errors(),
        doc: "Print every live mortal region (id, rc, object count, object tags) to stderr. Returns nil.",
        category: "debug",
        example: "(arena/dump)",
        aliases: &["arena/dump"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-region-count" => prim_arena_region_count {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return number of active regions.",
        category: "debug",
        example: "(debug/arena-region-count)",
        aliases: &["arena/region-count"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-region-ids" => prim_arena_region_ids {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return physical region ids issued (one past the largest id ever minted from scratch).",
        category: "debug",
        example: "(debug/arena-region-ids)",
        aliases: &["arena/region-ids"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-region-table" => prim_arena_region_table {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return entries in the region table (one past the largest physical region id ever made live).",
        category: "debug",
        example: "(debug/arena-region-table)",
        aliases: &["arena/region-table"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-region-info" => prim_arena_region_info {
        signal: Signal::errors(),
        doc: "Return array of {:id N :rc N :objects N} per active region.",
        category: "debug",
        example: "(debug/arena-region-info)",
        aliases: &["arena/region-info"],
        effect: RegionEffect::Fresh,
    }
}
