use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

use super::def::{Doc, PrimitiveDef, PrimitiveMeta};
use super::{
    allocator, arena, arithmetic, array, bitwise, bytes, chan, comparison, compile, concurrency,
    config, convert, debug, disassembly, display, fiber_introspect, fibers, fileio, format,
    intrinsics, introspection, io, json, list, loading, logic, lstruct, math, memory, meta,
    modules, net, package, parameters, path, ports, posix, r#box, read, sets, sort, stream, string,
    structs, subprocess, time, traits, types, unix, watch,
};

/// All primitive tables. Each module exports a `const PRIMITIVES`
/// array; this list is the single place that enumerates them.
///
/// Tables gated behind `ffi` are appended via `ffi_tables()` below
/// because `const` arrays cannot contain conditional entries.
pub(crate) const ALL_TABLES: &[&[PrimitiveDef]] = &[
    allocator::PRIMITIVES,
    arena::PRIMITIVES,
    arithmetic::PRIMITIVES,
    array::PRIMITIVES,
    bitwise::PRIMITIVES,
    bytes::PRIMITIVES,
    r#box::PRIMITIVES,
    chan::PRIMITIVES,
    compile::PRIMITIVES,
    config::PRIMITIVES,
    comparison::PRIMITIVES,
    convert::PRIMITIVES,
    concurrency::PRIMITIVES,
    debug::PRIMITIVES,
    disassembly::PRIMITIVES,
    display::PRIMITIVES,
    fiber_introspect::PRIMITIVES,
    fibers::PRIMITIVES,
    fileio::PRIMITIVES,
    format::PRIMITIVES,
    intrinsics::PRIMITIVES,
    introspection::PRIMITIVES,
    #[cfg(feature = "mlir")]
    introspection::MLIR_PRIMITIVES,
    io::PRIMITIVES,
    json::PRIMITIVES,
    list::PRIMITIVES,
    loading::PRIMITIVES,
    logic::PRIMITIVES,
    math::PRIMITIVES,
    memory::PRIMITIVES,
    meta::PRIMITIVES,
    modules::PRIMITIVES,
    net::PRIMITIVES,
    unix::PRIMITIVES,
    package::PRIMITIVES,
    parameters::PRIMITIVES,
    path::PRIMITIVES,
    ports::PRIMITIVES,
    posix::PRIMITIVES,
    subprocess::PRIMITIVES,
    read::PRIMITIVES,
    sets::PRIMITIVES,
    sort::PRIMITIVES,
    stream::PRIMITIVES,
    string::PRIMITIVES,
    structs::PRIMITIVES,
    lstruct::PRIMITIVES,
    time::PRIMITIVES,
    traits::PRIMITIVES,
    types::PRIMITIVES,
    watch::PRIMITIVES,
];

/// Primitive tables that require the `ffi` feature (libffi).
#[cfg(feature = "ffi")]
fn ffi_tables() -> &'static [&'static [PrimitiveDef]] {
    &[loading::CALLBACK_PRIMITIVES]
}

#[cfg(not(feature = "ffi"))]
fn ffi_tables() -> &'static [&'static [PrimitiveDef]] {
    &[]
}

/// Name→def index over every primitive table, including aliases.
///
/// Compile-time consumers (type inference reading `PrimitiveDef::ret`)
/// use this to look up primitive metadata by source name without a VM —
/// the same const tables `register_primitives` feeds, so the data cannot
/// drift from what runs.
pub(crate) fn def_by_name(name: &str) -> Option<&'static PrimitiveDef> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static INDEX: LazyLock<HashMap<&'static str, &'static PrimitiveDef>> = LazyLock::new(|| {
        let mut index = HashMap::new();
        for table in ALL_TABLES.iter().chain(ffi_tables().iter()) {
            for def in *table {
                index.insert(def.name, def);
                for alias in def.aliases {
                    index.insert(*alias, def);
                }
            }
        }
        index
    });
    INDEX.get(name).copied()
}

/// The process-global registry owning the `prim_id` ↔ `&'static PrimitiveDef`
/// correspondence. A native-fn is the IMMEDIATE `Value{TAG_NATIVE_FN, prim_id}`
/// (no `HeapObject`, no region), so the id is its whole identity: dense (a
/// switch key for tier-2 MLIR/GPU lowering) and portable across `send`/`spawn`.
///
/// Seeded with the canonical `ALL_TABLES`+ffi defs so the common primitives get
/// small, deterministic ids; any other native-fn def (the trait-method handlers
/// in `traitregistry`, ffi callbacks) is appended on first `prim_id_of` — in a
/// deterministic startup order, so the ids are stable across runs of one binary.
/// Keyed by the def's `&'static` address (every table is a `const PRIMITIVES`
/// reference into promoted static memory, so addresses are stable).
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
        for t in ALL_TABLES.iter().chain(ffi_tables().iter()) {
            for def in *t {
                let key = def as *const PrimitiveDef as usize;
                if !reg.by_ptr.contains_key(&key) {
                    let id = reg.defs.len() as u32;
                    reg.defs.push(def);
                    reg.by_ptr.insert(key, id);
                }
            }
        }
        std::sync::Mutex::new(reg)
    });

/// Visit the ordered canonical primitive-table identity (`name` + `aliases`)
/// into a hasher.
///
/// A native-fn immediate's whole payload is its `prim_id`, which is the def's
/// index in the canonical `ALL_TABLES`+`ffi_tables` enumeration. That id is
/// only meaningful against the exact table that minted it, so consumers that
/// persist native-fn values (the stdlib disk cache) must mix this identity
/// into their key: any addition, removal, rename, or reorder of a canonical
/// primitive changes the hash and invalidates the persisted payload.
pub(crate) fn hash_prim_table_identity<H: std::hash::Hasher>(hasher: &mut H) {
    for table in ALL_TABLES.iter().chain(ffi_tables().iter()) {
        for def in *table {
            hash_def_identity(def, hasher);
        }
    }
}

/// Hash one def's identity fields (see [`hash_prim_table_identity`]).
fn hash_def_identity<H: std::hash::Hasher>(def: &PrimitiveDef, hasher: &mut H) {
    use std::hash::Hash;
    def.name.hash(hasher);
    def.aliases.hash(hasher);
}

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

/// Register all primitive functions with the VM and build metadata.
pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for table in ALL_TABLES.iter().chain(ffi_tables().iter()) {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            let native_val = Value::native_fn(def);
            meta.signals.insert(sym_id, def.signal);
            meta.arities.insert(sym_id, def.arity);
            meta.functions.insert(sym_id, native_val);
            meta.effects.insert(sym_id, def.effect);
            meta.ret_types.insert(sym_id, def.ret);
            meta.embeds.insert(sym_id, def.embeds);
            meta.moves_out.insert(sym_id, def.moves_out);

            let doc = Doc {
                name: def.name,
                doc: def.doc,
                params: def.params,
                arity: def.arity,
                signal: def.signal,
                category: def.category,
                example: def.example,
                aliases: def.aliases,
            };
            vm.docs.insert(def.name.to_string(), doc.clone());

            for alias in def.aliases {
                let alias_id = symbols.intern(alias);
                let alias_val = Value::native_fn(def);
                meta.signals.insert(alias_id, def.signal);
                meta.arities.insert(alias_id, def.arity);
                meta.functions.insert(alias_id, alias_val);
                meta.effects.insert(alias_id, def.effect);
                meta.ret_types.insert(alias_id, def.ret);
                meta.embeds.insert(alias_id, def.embeds);
                meta.moves_out.insert(alias_id, def.moves_out);
                vm.docs.insert((*alias).to_string(), doc.clone());
            }
        }
    }

    super::docs::register_builtin_docs(&mut vm.docs);

    meta
}

/// Build primitive metadata without registering in a VM.
///
/// Iterates the same PRIMITIVES tables as `register_primitives` but
/// only builds the signals/arities maps. Used by pipeline functions
/// that receive an already-configured VM.
pub fn build_primitive_meta(symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for table in ALL_TABLES.iter().chain(ffi_tables().iter()) {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            meta.signals.insert(sym_id, def.signal);
            meta.arities.insert(sym_id, def.arity);
            meta.functions.insert(sym_id, Value::native_fn(def));
            meta.effects.insert(sym_id, def.effect);
            meta.ret_types.insert(sym_id, def.ret);
            meta.embeds.insert(sym_id, def.embeds);
            meta.moves_out.insert(sym_id, def.moves_out);

            for alias in def.aliases {
                let alias_id = symbols.intern(alias);
                meta.signals.insert(alias_id, def.signal);
                meta.arities.insert(alias_id, def.arity);
                meta.functions.insert(alias_id, Value::native_fn(def));
                meta.effects.insert(alias_id, def.effect);
                meta.ret_types.insert(alias_id, def.ret);
                meta.embeds.insert(alias_id, def.embeds);
                meta.moves_out.insert(alias_id, def.moves_out);
            }
        }
    }

    meta
}

/// Intern all primitive names (and aliases) into a SymbolTable.
///
/// This ensures the SymbolTable has the same SymbolId assignments as
/// the cached PrimitiveMeta. Must be called before using cached meta
/// with a SymbolTable that hasn't had `register_primitives` called on it.
/// Idempotent — safe to call multiple times.
pub fn intern_primitive_names(symbols: &mut SymbolTable) {
    for table in ALL_TABLES.iter().chain(ffi_tables().iter()) {
        for def in *table {
            symbols.intern(def.name);
            for alias in def.aliases {
                symbols.intern(alias);
            }
        }
    }
}

#[cfg(test)]
mod tests;
