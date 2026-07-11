//! Dynamic parameter baseline a child fiber inherits from its parent on first
//! resume, plus the debug-only uncounted-borrow check that guards it. Seeding
//! the baseline takes no reference count, so `record_param_borrows` snapshots
//! each borrowed region's generation and `first_stale_borrow` later confirms
//! none was freed under the borrow (docs/impl/region/generations.md
//! § "Uncounted-borrow check").

use crate::value::Value;

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
