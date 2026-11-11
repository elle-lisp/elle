// audited: 2026-11-11
//! The process-global registry: the `prim_id` a native-fn immediate carries, and the def it names.
//!
//! docs/impl/values.md

use super::def::PrimitiveDef;
use super::registration::canonical_defs;

/// The process-global registry owning the `prim_id` ↔ `&'static PrimitiveDef`
/// correspondence. A native-fn is the IMMEDIATE `Value{TAG_NATIVE_FN, prim_id}`
/// (no `HeapObject`, no region), so the id is its whole identity: dense (a
/// switch key for tier-2 MLIR/GPU lowering) and portable across `send`/`spawn`.
///
/// Seeded with the canonical `ALL_TABLES`+ffi defs so the common primitives get
/// small, deterministic ids; any other native-fn def (the trait-method handlers
/// in `traitregistry`, ffi callbacks) is appended on first `prim_id_of` — in a
/// deterministic startup order, so the ids are stable across runs of one binary.
/// Keyed by the def's `&'static` address (every table is a `static PRIMITIVES`
/// item, so addresses are stable across every reference site).
struct PrimRegistry {
    defs: Vec<&'static PrimitiveDef>,
    by_ptr: std::collections::HashMap<usize, u32>,
}

static PRIM_REGISTRY: std::sync::LazyLock<std::sync::Mutex<PrimRegistry>> =
    std::sync::LazyLock::new(|| {
        let mut reg = PrimRegistry {
            defs: Vec::new(),
            by_ptr: std::collections::HashMap::new(),
        };
        for def in canonical_defs() {
            let key = def as *const PrimitiveDef as usize;
            if !reg.by_ptr.contains_key(&key) {
                let id = reg.defs.len() as u32;
                reg.defs.push(def);
                reg.by_ptr.insert(key, id);
            }
        }
        std::sync::Mutex::new(reg)
    });

/// The `prim_id` of a primitive definition — how `Value::native_fn` finds the id
/// to store as the immediate payload. Returns the existing id, or appends the def
/// and assigns the next one (for defs minted outside the canonical tables, e.g.
/// trait-method handlers).
pub fn prim_id_of(def: &'static PrimitiveDef) -> u32 {
    let mut reg = PRIM_REGISTRY.lock().expect("prim registry poisoned");
    let key = def as *const PrimitiveDef as usize;
    if let Some(&id) = reg.by_ptr.get(&key) {
        return id;
    }
    let id = reg.defs.len() as u32;
    reg.defs.push(def);
    reg.by_ptr.insert(key, id);
    id
}

/// The primitive def for a `prim_id` — the inverse of [`prim_id_of`], used by
/// `Value::as_native_def` to resolve an immediate native-fn back to its def.
/// `None` for an id never assigned (a corrupt/foreign payload).
pub fn prim_def(id: u32) -> Option<&'static PrimitiveDef> {
    PRIM_REGISTRY
        .lock()
        .expect("prim registry poisoned")
        .defs
        .get(id as usize)
        .copied()
}

/// An owned snapshot of the canonical prim table (`prim_id` = index). For a
/// consumer that needs an indexable table which AGREES with the immediate
/// native-fn `prim_id` payloads — the WASM host's dispatch table. Take it after
/// startup registration, when every primitive (core, ffi, trait-method) is
/// interned.
pub fn prim_table_snapshot() -> Vec<&'static PrimitiveDef> {
    PRIM_REGISTRY
        .lock()
        .expect("prim registry poisoned")
        .defs
        .clone()
}

/// Hold the lock `prim_id_of` takes, for a test of what waits on it.
#[cfg(test)]
fn lock_writers() -> std::sync::MutexGuard<'static, PrimRegistry> {
    PRIM_REGISTRY.lock().expect("prim registry poisoned")
}

#[cfg(test)]
mod tests;
