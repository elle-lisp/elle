// audited: 2026-10-07
//! Closure call counts by bytecode address, each valid only while its function's code region lives.
//!
//! docs/impl/jit.md

use rustc_hash::FxHashMap;

use crate::value::fiberheap::FiberHeap;
use crate::value::ClosureTemplate;

/// One function's count, and the code region whose generation says whether
/// the function is still there (docs/impl/region/generations.md).
#[derive(Clone, Copy)]
struct Count {
    region: u32,
    generation: u32,
    calls: usize,
}

impl Count {
    /// Whether the function this count began for still lives: its code
    /// region has not been freed since, which would have moved the generation.
    fn live(&self, heap: &FiberHeap) -> bool {
        heap.region_generation(self.region) == self.generation
    }
}

/// The size below which the table never sweeps.
const FIRST_SWEEP: usize = 1024;

/// The VM's call counts, keyed by a code object's bytecode address.
///
/// A count holds no pin, because every function a counting tier calls gets
/// one. It records its code region and that region's generation instead, so a
/// count whose function was freed reads as zero, and a later function at the
/// same address starts fresh. The table drops dead counts each time it doubles
/// in size.
pub struct CallCounts {
    counts: FxHashMap<*const u8, Count>,
    /// The size at which the next new count first sweeps the dead ones out.
    sweep_at: usize,
}

impl Default for CallCounts {
    fn default() -> Self {
        CallCounts {
            counts: FxHashMap::default(),
            sweep_at: FIRST_SWEEP,
        }
    }
}

impl CallCounts {
    /// Count one call of the code object `t`, on the heap that holds it, and
    /// answer its count including this call.
    pub fn record(&mut self, heap: &FiberHeap, t: &ClosureTemplate) -> usize {
        let key = t.bytecode().as_ptr();
        if let Some(count) = self.counts.get_mut(&key) {
            if count.live(heap) {
                count.calls += 1;
                return count.calls;
            }
        }
        if self.counts.len() >= self.sweep_at {
            self.counts.retain(|_, count| count.live(heap));
            self.sweep_at = FIRST_SWEEP.max(2 * self.counts.len());
        }
        let region = heap.region_of_ptr(t.payload_backing());
        let count = Count {
            region,
            generation: heap.region_generation(region),
            calls: 1,
        };
        self.counts.insert(key, count);
        1
    }

    /// The count of the function at `key`, or 0 when no count names a live
    /// function there.
    pub fn get(&self, heap: &FiberHeap, key: *const u8) -> usize {
        self.counts
            .get(&key)
            .filter(|count| count.live(heap))
            .map_or(0, |count| count.calls)
    }

    /// How many counts the table holds, live or not yet swept.
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Drop every count.
    pub fn clear(&mut self) {
        self.counts.clear();
        self.sweep_at = FIRST_SWEEP;
    }
}
