//! The heap's code-payload cache.
//!
//! One payload per blueprint, materialized on first use and shared by every
//! header built from it afterwards (docs/impl/region/template.md § "Who owns
//! the payload region"). Instance-owned rider state on `FiberHeap`, beside the
//! root region and the process-root registry — two coexisting instances each
//! materialize their own payloads.

use std::rc::{Rc, Weak};

use rustc_hash::FxHashMap;

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::region_slice::RegionSlice;

use super::payload::CodePayload;
use super::proto::{materialize_payload, TemplateProto};

/// Payload bytes after which the open payload region is closed and the next
/// blueprint starts a fresh one. Packing several blueprints into one region
/// gives a compile unit one region instead of a page per lambda; the threshold
/// bounds how long a short-lived blueprint's payload can pin a long-lived
/// one's.
const PAYLOAD_REGION_BYTES: usize = 256 * 1024;

/// Entries below which a sweep is not worth walking the map.
const SWEEP_FLOOR: usize = 64;

/// One region holding the payloads of several blueprints.
///
/// Slots are never removed, only marked released: an entry names its region by
/// index, and compacting the list would silently repoint every entry after the
/// hole at another blueprint's region.
struct PayloadRegion {
    region: RuntimeRegion,
    /// Payload bytes materialized into it so far.
    bytes: usize,
    /// How many cache entries name it. The region is released when the last
    /// one goes.
    live: usize,
    released: bool,
}

/// One blueprint's materialized payload.
struct Entry {
    /// The blueprint this payload was built for. Not an optimization: Rust
    /// reuses the address of a freed allocation, so a key match alone would
    /// hand a new blueprint the dead one's code.
    proto: Weak<TemplateProto>,
    region_ix: usize,
    payload: RegionSlice<CodePayload>,
}

/// Every payload this instance has materialized.
#[derive(Default)]
pub(crate) struct TemplatePayloads {
    regions: Vec<PayloadRegion>,
    /// Blueprint address → its payload.
    entries: FxHashMap<usize, Entry>,
    /// Entry count at the last sweep; the next runs when the map doubles.
    swept_at: usize,
    /// This instance's placeholder code object, minted on first use.
    placeholder: Option<(Rc<TemplateProto>, crate::value::Value)>,
}

impl TemplatePayloads {
    /// The payload cached for `proto`, if the entry still names it.
    fn get(&self, proto: &Rc<TemplateProto>) -> Option<RegionSlice<CodePayload>> {
        let entry = self.entries.get(&(Rc::as_ptr(proto) as usize))?;
        // The weak keeps the old allocation readable, so a dead blueprint whose
        // address was reused reports a strong count of zero here rather than
        // aliasing the new one.
        (entry.proto.strong_count() > 0).then_some(entry.payload)
    }
}

impl FiberHeap {
    /// This blueprint's payload, materializing it on first use.
    ///
    /// The payload lands in a payload region of this heap's own, so the header
    /// that names it takes a counted cross-region reference at its allocation
    /// and the free cascade releases it.
    pub(crate) fn template_payload(
        &mut self,
        proto: &Rc<TemplateProto>,
    ) -> RegionSlice<CodePayload> {
        if let Some(payload) = self.template_payloads.get(proto) {
            return payload;
        }
        self.sweep_template_payloads_if_due();

        let (region_ix, region) = self.open_payload_region();
        let before = self.region_bytes(region);
        let payload = materialize_payload(self, proto, region);
        let grew = self.region_bytes(region).saturating_sub(before);

        let cache = &mut self.template_payloads;
        cache.regions[region_ix].bytes += grew;
        cache.regions[region_ix].live += 1;
        cache.entries.insert(
            Rc::as_ptr(proto) as usize,
            Entry {
                proto: Rc::downgrade(proto),
                region_ix,
                payload,
            },
        );
        payload
    }

    /// This instance's placeholder code object: a nullary body of a single
    /// `Return` that is never executed.
    ///
    /// A fiber that runs no bytecode still names a code object — the root
    /// fiber, whose execution context is top-level bytecode rather than a
    /// closure, and a native-iterator fiber, which the resume path
    /// short-circuits. A code object is a region allocation, so the instance
    /// mints one and shares it rather than each site inventing another. It
    /// lives in the pinned root region, so it is valid for the instance's life.
    pub(crate) fn placeholder_template(&mut self) -> super::TemplateRef {
        if let Some((_, value)) = &self.template_payloads.placeholder {
            return super::TemplateRef::region(*value);
        }
        let proto = Rc::new(TemplateProto::new(
            vec![
                crate::compiler::bytecode::Instruction::Return as u8,
                0,
                0,
                0,
            ],
            crate::value::types::Arity::Exact(0),
            Vec::new(),
        ));
        let region = crate::value::arena::root_region(self);
        let value = super::materialize(self, &proto, region);
        self.template_payloads.placeholder = Some((proto, value));
        super::TemplateRef::region(value)
    }

    /// The open payload region, or a fresh one when the open one is full.
    fn open_payload_region(&mut self) -> (usize, RuntimeRegion) {
        if let Some(ix) = self
            .template_payloads
            .regions
            .iter()
            .rposition(|r| !r.released && r.bytes < PAYLOAD_REGION_BYTES)
        {
            return (ix, self.template_payloads.regions[ix].region);
        }
        let region = self.new_runtime_region();
        self.template_payloads.regions.push(PayloadRegion {
            region,
            bytes: 0,
            live: 0,
            released: false,
        });
        (self.template_payloads.regions.len() - 1, region)
    }

    /// Drop the entries whose blueprint has died and release any payload region
    /// left with none. A header holds its blueprint strongly, so a swept entry
    /// has no live header reading its payload.
    pub(crate) fn release_dead_template_payloads(&mut self) {
        let cache = &mut self.template_payloads;
        let dead: Vec<usize> = cache
            .entries
            .iter()
            .filter(|(_, e)| e.proto.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut emptied: Vec<RuntimeRegion> = Vec::new();
        for key in dead {
            let entry = cache.entries.remove(&key).expect("just enumerated");
            let region = &mut cache.regions[entry.region_ix];
            region.live -= 1;
            // A region whose last blueprint is gone is released, but its slot
            // stays: entries name their region by index.
            if region.live == 0 && !region.released {
                region.released = true;
                emptied.push(region.region);
            }
        }
        cache.swept_at = cache.entries.len();
        for region in emptied {
            self.decref_region_if_present(region);
        }
    }

    /// Release every payload region, live entries included. The teardown sweep
    /// calls this: nothing may still be executing, so every payload is dead
    /// whatever its blueprint's refcount says.
    pub(crate) fn release_all_template_payloads(&mut self) {
        let cache = std::mem::take(&mut self.template_payloads);
        for region in cache.regions {
            if !region.released {
                self.decref_region_if_present(region.region);
            }
        }
    }

    fn sweep_template_payloads_if_due(&mut self) {
        let cache = &self.template_payloads;
        if cache.entries.len() >= cache.swept_at.max(SWEEP_FLOOR) * 2 {
            self.release_dead_template_payloads();
        }
    }

    /// Bytes committed to `region`'s pages — how the cache measures a payload
    /// against its region's threshold.
    fn region_bytes(&self, region: RuntimeRegion) -> usize {
        self.region_pool(region)
            .map(|p| p.allocated_bytes())
            .unwrap_or(0)
    }
}
