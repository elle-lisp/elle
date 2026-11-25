// audited: 2026-11-25
//! The fiber discharge: what a dying fiber still holds joins its free's cascade.
//!
//! docs/impl/region/owner.md
//! docs/impl/region/park.md

use super::*;

impl RegionStore {
    /// Phase 2b of the wholesale free (docs/impl/region/owner.md). A dying
    /// `Fiber` object whose fiber was never routed through a terminal
    /// transition (a dropped handle, a still-paused or `:error` fiber) still
    /// holds state. That is its parked chain's activation owner nodes, its own
    /// fiber node, and a parked non-terminal signal's park escape retain. Take
    /// that state out of each dying fiber, and push its regions onto `frontier`
    /// for the same iterative cascade as the recorded edges. The caller runs
    /// this AFTER the equivalence oracle: these are not content edges (no record
    /// exists), so they must not take part in the recorded == scanned
    /// comparison. A borrowed (executing) fiber is skipped — its region cannot be
    /// dying while it runs.
    pub(super) fn discharge_fibers(
        &self,
        members: &[(RuntimeRegion, RegionEntry)],
        frontier: &mut Vec<u32>,
    ) {
        let start = frontier.len();
        for (_, entry) in members {
            for obj in entry.pool.live_objects() {
                let HeapObject::Fiber { handle, .. } = obj else {
                    continue;
                };
                handle.try_with_mut(|fib| {
                    let parked = fib.take_parked_state();
                    frontier.extend(parked.dues.nodes.iter().map(|r| r.get()));
                    // The releases those activations took over from their own
                    // frame-replacing tail calls (docs/impl/region/owner.md).
                    // The completion that would have run them is a
                    // continuation this fiber can never re-enter. Unlike the
                    // owed tables below, these name no value the payload could
                    // be: a deferred region is the tail callee's own closure
                    // region or a merged arena, and a raise's payload is
                    // neither.
                    frontier.extend(parked.dues.deferred.iter().map(|r| r.get()));
                    if let Some(node) = fib.fiber_owner_node.take() {
                        frontier.push(node.get());
                    }
                    // The releases the parked frames still owed, off their own
                    // value-route slots (docs/impl/region/mechanism.md). A frame
                    // this fiber can never re-enter never reaches the route that
                    // would have dropped them. Resolved exactly as the parked
                    // signal below is, and skipping the payload's own region: that
                    // value leaves as the fiber's result.
                    let protect = parked
                        .protect
                        .and_then(|v| v.as_heap_ptr().map(|ptr| self.owning_region(ptr)));
                    for v in &parked.dues.owed {
                        let Some(ptr) = v.as_heap_ptr() else {
                            continue;
                        };
                        let rid = self.owning_region(ptr);
                        if rid > 1 && protect != Some(rid) {
                            frontier.push(rid);
                        }
                    }
                    // The slot-routed half. The establishing generation is
                    // checked first: a mapping whose region has since been
                    // freed and recycled is a leftover the frame's own release
                    // already answered for.
                    for m in &parked.dues.owed_regions {
                        let rid = m.region.get();
                        let live = self.generation_raw(rid) == m.gen
                            && self.regions.get(rid as usize).is_some_and(|s| s.is_some());
                        if rid > 1 && live && protect != Some(rid) {
                            frontier.push(rid);
                        }
                    }
                    // The parked non-terminal signal's park escape retain
                    // (EmitEscape / SuspendEscape), released at a resume
                    // that will never come. Resolve the value's region
                    // exactly as the content scan does (`owning_region`); a
                    // foreign or immediate value resolves to nothing.
                    if let Some((_, v)) = parked.signal {
                        if let Some(ptr) = v.as_heap_ptr() {
                            let rid = self.owning_region(ptr);
                            if rid > 1 {
                                frontier.push(rid);
                            }
                        }
                    }
                });
            }
        }
        if crate::config::get().has_trace("rc") && frontier.len() > start {
            eprintln!("[trace:rc] fiber_discharge → {:?}", &frontier[start..]);
        }
    }
}
