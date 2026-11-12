// audited: 2026-11-11
//! The process-global registry: the `prim_id` a native-fn immediate carries, and the def it names.
//!
//! docs/impl/values.md

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};

use super::def::PrimitiveDef;
use super::registration::canonical_defs;

/// The slot count of the first segment. Segment `k` holds `FIRST << k` slots.
const FIRST: usize = 64;

/// Enough segments to give every `u32` a slot.
const SEGMENTS: usize = (u32::MAX as usize / FIRST + 1).ilog2() as usize + 1;

/// One id's def: written once, under the writer lock, and read without it.
type Slot = OnceLock<&'static PrimitiveDef>;

/// The process-global registry owning the `prim_id` ↔ `&'static PrimitiveDef`
/// correspondence. A native-fn is the IMMEDIATE `Value{TAG_NATIVE_FN, prim_id}`
/// (no `HeapObject`, no region), so the id is its whole identity: dense (a
/// switch key for tier-2 MLIR/GPU lowering) and portable across `send`/`spawn`.
///
/// Seeded with the canonical `ALL_TABLES`+ffi defs so the common primitives get
/// small, deterministic ids; any other native-fn def (the trait-method handlers
/// in `traitregistry`, plugin primitives, ffi callbacks) is appended on first
/// `prim_id_of` — in a deterministic startup order, so the ids are stable
/// across runs of one binary. Keyed by the def's `&'static` address (every
/// table is a `static PRIMITIVES` item, so addresses are stable across every
/// reference site).
///
/// The defs sit in segments that double in size and never move once
/// allocated, so a reader reaches a def through atomic loads alone. The lock
/// guards the address index alone, and a def's slot is filled before
/// `prim_id_of` hands out its id.
struct PrimRegistry {
    segments: [OnceLock<Box<[Slot]>>; SEGMENTS],
    /// Each def's address to its id. Its length is the next id.
    ids: Mutex<HashMap<usize, u32>>,
}

impl PrimRegistry {
    fn seeded() -> Self {
        let mut registry = PrimRegistry {
            segments: std::array::from_fn(|_| OnceLock::new()),
            ids: Mutex::new(HashMap::new()),
        };
        let mut ids = HashMap::new();
        for def in canonical_defs() {
            registry.id_of(&mut ids, def);
        }
        registry.ids = Mutex::new(ids);
        registry
    }

    /// The id of `def`, appended to `ids` and to its slot if it has none yet.
    fn id_of(&self, ids: &mut HashMap<usize, u32>, def: &'static PrimitiveDef) -> u32 {
        let key = def as *const PrimitiveDef as usize;
        if let Some(&id) = ids.get(&key) {
            return id;
        }
        let id = u32::try_from(ids.len()).expect("a prim_id for every u32");
        let (segment, offset) = slot_of(id);
        let slots = self.segments[segment]
            .get_or_init(|| (0..FIRST << segment).map(|_| Slot::new()).collect());
        assert!(slots[offset].set(def).is_ok(), "prim_id {id} filled twice");
        ids.insert(key, id);
        id
    }

    fn def(&self, id: u32) -> Option<&'static PrimitiveDef> {
        let (segment, offset) = slot_of(id);
        self.segments[segment].get()?[offset].get().copied()
    }
}

/// The segment that holds `id`, and the offset of its slot in that segment.
fn slot_of(id: u32) -> (usize, usize) {
    let segment = (id as usize / FIRST + 1).ilog2() as usize;
    (segment, id as usize - FIRST * ((1 << segment) - 1))
}

static PRIM_REGISTRY: LazyLock<PrimRegistry> = LazyLock::new(PrimRegistry::seeded);

/// The `prim_id` of a primitive definition — how `Value::native_fn` finds the id
/// to store as the immediate payload. Returns the existing id, or appends the def
/// and assigns the next one (for defs minted outside the canonical tables, e.g.
/// trait-method handlers).
pub fn prim_id_of(def: &'static PrimitiveDef) -> u32 {
    let mut ids = PRIM_REGISTRY.ids.lock().expect("prim registry poisoned");
    PRIM_REGISTRY.id_of(&mut ids, def)
}

/// The primitive def for a `prim_id` — the inverse of [`prim_id_of`], used by
/// `Value::as_native_def` to resolve an immediate native-fn back to its def.
/// It takes no lock. `None` for an id never assigned (a corrupt/foreign payload).
pub fn prim_def(id: u32) -> Option<&'static PrimitiveDef> {
    PRIM_REGISTRY.def(id)
}

/// An owned snapshot of the canonical prim table (`prim_id` = index). For a
/// consumer that needs an indexable table which AGREES with the immediate
/// native-fn `prim_id` payloads — the WASM host's dispatch table. Take it after
/// startup registration, when every primitive (core, ffi, trait-method) is
/// interned.
pub fn prim_table_snapshot() -> Vec<&'static PrimitiveDef> {
    (0..=u32::MAX).map_while(prim_def).collect()
}

/// Hold the lock `prim_id_of` takes, for a test of what waits on it.
#[cfg(test)]
fn lock_writers() -> std::sync::MutexGuard<'static, HashMap<usize, u32>> {
    PRIM_REGISTRY.ids.lock().expect("prim registry poisoned")
}

#[cfg(test)]
mod tests;
