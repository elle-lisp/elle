//! Dynamic parameter baseline a child fiber inherits from its parent — the
//! counted seeding (`retain_param_baseline`) and the debug-only borrow check
//! that proves the count holds. A child routinely outlives its creator's
//! `parameterize` scope, so each heap entry of the installed baseline takes
//! one retain and a recorded `fiber → value` content edge, released by the
//! fiber object's own free (docs/impl/region/owner.md § "A child's inherited
//! parameter baseline is a counted holder"). `record_param_borrows` snapshots
//! each entry region's generation and `first_stale_borrow` later confirms
//! none was freed under the fiber (docs/impl/region/generations.md
//! § "Uncounted-borrow check").

use crate::value::Value;

/// Retain a seeded parameter baseline: one `ParamBaseline` retain and one
/// recorded `fiber → value` content edge per heap entry. Called once per
/// fiber, right after the baseline frame is installed, with the fiber's own
/// heap VALUE (the edges' source). The symmetric release is the Fiber
/// content-scan arm's baseline walk at the fiber object's free — gated on
/// `Fiber::param_baseline_seeded`, which every install site must set.
pub(crate) fn retain_param_baseline(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: Value,
    flat: &[(u32, Value)],
) {
    let fiber_r = crate::value::arena::region_of(heap, fiber_value);
    for &(_, v) in flat {
        let r = crate::value::arena::region_of(heap, v);
        // A value co-region with the fiber object needs no edge and MUST take
        // no retain: the free-cascade scan skips self-region refs, so a
        // self-retain here would keep the fiber's own region alive forever.
        if r.is_some() && r != fiber_r {
            crate::value::arena::incref_for_escape(
                heap,
                r,
                crate::value::arena::EscapeSite::ParamBaseline,
            );
            heap.record_outgoing_edge(fiber_r, r);
        }
    }
}

/// Flatten dynamic parameter frames into a single baseline frame, later
/// frames overriding earlier ones. Used when a child fiber inherits its
/// parent's dynamic bindings on first resume.
pub(crate) fn flatten_param_frames(frames: &[Vec<(u32, Value)>]) -> Vec<(u32, Value)> {
    let mut flat: Vec<(u32, Value)> = Vec::new();
    for frame in frames {
        for &(id, val) in frame {
            if let Some(pos) = flat.iter().position(|&(k, _)| k == id) {
                flat[pos].1 = val;
            } else {
                flat.push((id, val));
            }
        }
    }
    flat
}

/// Snapshot the uncounted cross-fiber borrows in a child's inherited baseline
/// parameter frame: for each heap-valued binding, record `(param_id, region,
/// current generation)`. Seeding the baseline takes no reference count, so this
/// records the generation at which each borrowed region is live; the resume and
/// `resolve_parameter` checks later confirm the region has not been freed since.
/// Immediate (non-heap) bindings carry no region and are skipped. Debug-only —
/// the check it feeds runs only under `debug_assertions`
/// (docs/impl/region/generations.md § "Uncounted-borrow check").
///
/// The region and its generation are read from the SAME `heap`
/// (`region_of_ptr`/`generation_raw`), so the recorded pair and the later
/// check compare generations within one store — never across stores, where
/// generations are unrelated numbers.
#[cfg(debug_assertions)]
pub(crate) fn record_param_borrows(
    flat: &[(u32, Value)],
    heap: &crate::value::fiberheap::FiberHeap,
) -> Vec<(u32, crate::hir::region::RuntimeRegion, u32)> {
    flat.iter()
        .filter_map(|&(pid, v)| {
            if !v.is_heap() {
                return None;
            }
            let ptr = v.as_heap_ptr()?;
            let r = crate::hir::region::RuntimeRegion::new(heap.region_of_ptr(ptr))?;
            Some((pid, r, heap.generation_raw(r.get())))
        })
        .collect()
}

/// The first recorded borrow whose region's generation has moved since it was
/// snapshotted — i.e. the region's pages were freed (and possibly recycled)
/// while a fiber still borrowed a value in it. Reads only the generation
/// counter, never the borrowed value's page, so it is sound to call after the
/// region was freed (a re-claimed page would pass `region_of`'s stamp check;
/// the recorded generation catches it). `None` when every borrow is still live.
#[cfg(debug_assertions)]
pub(crate) fn first_stale_borrow(
    borrows: &[(u32, crate::hir::region::RuntimeRegion, u32)],
    heap: &crate::value::fiberheap::FiberHeap,
) -> Option<(u32, crate::hir::region::RuntimeRegion)> {
    borrows
        .iter()
        .find(|&&(_, r, gen)| heap.generation_raw(r.get()) != gen)
        .map(|&(pid, r, _)| (pid, r))
}
