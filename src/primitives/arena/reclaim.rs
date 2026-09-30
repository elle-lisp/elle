// audited: 2026-09-29
//! The reclamation gauges: how a heap's regions have ended, one integer per primitive.
//!
//! docs/impl/region/diagnostics.md

use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

/// Write one primitive that answers the named field of the heap's reclamation
/// counters. Every gauge but `arena/owned` is this shape.
macro_rules! counter_primitive {
    ($prim:ident, $field:ident) => {
        pub(crate) fn $prim(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
            let n = ctx.heap_mut().reclaim_counters().$field;
            (SIG_OK, Value::int(n as i64))
        }
    };
}

counter_primitive!(prim_region_frees, region_frees);
counter_primitive!(prim_page_frees, page_frees);
counter_primitive!(prim_object_frees, object_frees);
counter_primitive!(prim_adopts, adopts);
counter_primitive!(prim_adopts_into_empty, adopts_into_empty);
counter_primitive!(prim_owned_frees, owned_frees);
counter_primitive!(prim_owned_free_pages, owned_free_pages);
counter_primitive!(prim_owned_free_objects, owned_free_objects);
counter_primitive!(prim_owned_one_page_frees, owned_one_page_frees);
counter_primitive!(prim_rescues, rescues);
counter_primitive!(prim_rescue_survivors, rescue_survivors);
counter_primitive!(prim_extracts, extracts);
counter_primitive!(prim_reparents, reparents);

/// (arena/owned) — the regions owned now: a reading, not a count.
pub(crate) fn prim_owned(ctx: &mut NativeCtx<'_>, _args: &[Value]) -> (SignalBits, Value) {
    let n = ctx.heap_mut().owned_count();
    (SIG_OK, Value::int(n as i64))
}

primitive! {
    "debug/arena-region-frees" => prim_region_frees {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return regions freed by a count reaching zero, an owner's drop, or a group free (monotonic).",
        category: "debug",
        example: "(debug/arena-region-frees)",
        aliases: &["arena/region-frees"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-page-frees" => prim_page_frees {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the pages the freed regions held (monotonic).",
        category: "debug",
        example: "(debug/arena-page-frees)",
        aliases: &["arena/page-frees"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-object-frees" => prim_object_frees {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the objects the freed regions held (monotonic).",
        category: "debug",
        example: "(debug/arena-object-frees)",
        aliases: &["arena/object-frees"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-adopts" => prim_adopts {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return adoptions: counted regions made members of an owner's subtree (monotonic).",
        category: "debug",
        example: "(debug/arena-adopts)",
        aliases: &["arena/adopts"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-adopts-into-empty" => prim_adopts_into_empty {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return adoptions into an owner that held no object (monotonic).",
        category: "debug",
        example: "(debug/arena-adopts-into-empty)",
        aliases: &["arena/adopts-into-empty"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-owned-frees" => prim_owned_frees {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return owned regions freed by their owner's drop (monotonic).",
        category: "debug",
        example: "(debug/arena-owned-frees)",
        aliases: &["arena/owned-frees"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-owned-free-pages" => prim_owned_free_pages {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the pages the owned regions freed by their owner's drop held (monotonic).",
        category: "debug",
        example: "(debug/arena-owned-free-pages)",
        aliases: &["arena/owned-free-pages"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-owned-free-objects" => prim_owned_free_objects {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the objects the owned regions freed by their owner's drop held (monotonic).",
        category: "debug",
        example: "(debug/arena-owned-free-objects)",
        aliases: &["arena/owned-free-objects"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-owned-one-page-frees" => prim_owned_one_page_frees {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return owned regions freed while they held one page or none (monotonic).",
        category: "debug",
        example: "(debug/arena-owned-one-page-frees)",
        aliases: &["arena/owned-one-page-frees"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-rescues" => prim_rescues {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return owned regions the drop-time rescue returned to counted (monotonic).",
        category: "debug",
        example: "(debug/arena-rescues)",
        aliases: &["arena/rescues"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-rescue-survivors" => prim_rescue_survivors {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return regions that outlived their owner's drop through a rescue, with the subtrees they keep (monotonic).",
        category: "debug",
        example: "(debug/arena-rescue-survivors)",
        aliases: &["arena/rescue-survivors"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-extracts" => prim_extracts {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return owned regions a moves-out removal returned to counted (monotonic).",
        category: "debug",
        example: "(debug/arena-extracts)",
        aliases: &["arena/extracts"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-reparents" => prim_reparents {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return owned regions handed from one owner to another (monotonic).",
        category: "debug",
        example: "(debug/arena-reparents)",
        aliases: &["arena/reparents"],
        effect: RegionEffect::Immediate,
    }
    "debug/arena-owned" => prim_owned {
        ret: RetType::Int,
        signal: Signal::errors(),
        doc: "Return the regions owned now (a reading, not a count).",
        category: "debug",
        example: "(debug/arena-owned)",
        aliases: &["arena/owned"],
        effect: RegionEffect::Immediate,
    }
}
