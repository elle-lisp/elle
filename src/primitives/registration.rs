// audited: 2026-11-11
//! Primitive registration: the canonical table list, the name and id indexes
//! over it, and the startup pass that installs every def into a VM.
//!
//! docs/cookbook/primitives.md

use crate::symbol::SymbolTable;
use crate::vm::VM;

use super::def::{Doc, PrimitiveDef, PrimitiveMeta};
use super::{
    allocator, arena, arithmetic, array, bitwise, bytes, chan, comparison, compile, concurrency,
    config, convert, debug, disassembly, display, fiber_introspect, fibers, fileio, format,
    intrinsics, introspection, io, json, list, loading, logic, lstruct, math, memory, meta,
    modules, net, package, parameters, path, ports, posix, r#box, read, sets, signature, sort,
    stream, string, structs, subprocess, time, traits, types, unix, watch,
};

/// All primitive tables. Each module exports a `static PRIMITIVES`
/// array; this list is the single place that enumerates them.
///
/// Tables gated behind `ffi` are appended via `ffi_tables()` below
/// because `static` arrays cannot contain conditional entries.
pub(crate) static ALL_TABLES: &[&[PrimitiveDef]] = &[
    allocator::PRIMITIVES,
    arena::PRIMITIVES,
    arena::reclaim::PRIMITIVES,
    arithmetic::PRIMITIVES,
    array::PRIMITIVES,
    bitwise::PRIMITIVES,
    bytes::PRIMITIVES,
    bytes::find::PRIMITIVES,
    r#box::PRIMITIVES,
    chan::PRIMITIVES,
    compile::PRIMITIVES,
    config::PRIMITIVES,
    comparison::PRIMITIVES,
    convert::PRIMITIVES,
    concurrency::PRIMITIVES,
    concurrency::handle::PRIMITIVES,
    debug::PRIMITIVES,
    disassembly::PRIMITIVES,
    display::PRIMITIVES,
    fiber_introspect::PRIMITIVES,
    fibers::PRIMITIVES,
    fileio::PRIMITIVES,
    fileio::content::PRIMITIVES,
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
    signature::PRIMITIVES,
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
    static TABLES: &[&[PrimitiveDef]] = &[loading::CALLBACK_PRIMITIVES];
    TABLES
}

#[cfg(not(feature = "ffi"))]
fn ffi_tables() -> &'static [&'static [PrimitiveDef]] {
    &[]
}

/// Every def in the canonical tables, `ALL_TABLES` then the ffi tables, in
/// table order.
pub(crate) fn canonical_defs() -> impl Iterator<Item = &'static PrimitiveDef> {
    ALL_TABLES
        .iter()
        .chain(ffi_tables().iter())
        .flat_map(|table| table.iter())
}

/// Name→def index over every primitive table, including aliases.
///
/// Compile-time consumers (type inference reading `PrimitiveDef::ret`)
/// use this to look up primitive metadata by source name without a VM —
/// the same static tables `register_primitives` feeds, so the data cannot
/// drift from what runs.
pub(crate) fn def_by_name(name: &str) -> Option<&'static PrimitiveDef> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static INDEX: LazyLock<HashMap<&'static str, &'static PrimitiveDef>> = LazyLock::new(|| {
        let mut index = HashMap::new();
        for def in canonical_defs() {
            index.insert(def.name, def);
            for alias in def.aliases {
                index.insert(*alias, def);
            }
        }
        index
    });
    INDEX.get(name).copied()
}

/// Hash→(spelling, def) index over the same tables `def_by_name` walks.
///
/// The built-in vocabulary is fixed at build time, so its spellings need no
/// instance memo: a compiler pass holding a primitive binding's `SymbolId` can
/// recover the name it was written with, wherever it runs and with no table in
/// scope. That is what keeps type inference, narrowing and fusion off the
/// display path (docs/impl/symbol.md).
///
/// `static_vocabulary_is_collision_free` asserts the spellings are distinct, so
/// a build whose own names collide fails before this index can hide one.
fn static_index() -> &'static std::collections::HashMap<u64, (&'static str, &'static PrimitiveDef)>
{
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static INDEX: LazyLock<HashMap<u64, (&'static str, &'static PrimitiveDef)>> =
        LazyLock::new(|| {
            let mut index = HashMap::new();
            for def in canonical_defs() {
                for name in std::iter::once(&def.name).chain(def.aliases) {
                    index.insert(crate::namehash::name_hash(name), (*name, def));
                }
            }
            index
        });
    &INDEX
}

/// The spelling of a built-in name, or `None` if `id` names nothing built in.
pub(crate) fn static_name(id: crate::value::SymbolId) -> Option<&'static str> {
    static_index().get(&id.0).map(|(name, _)| *name)
}

/// The def a built-in name resolves to, by id rather than by spelling.
pub(crate) fn def_by_symbol(id: crate::value::SymbolId) -> Option<&'static PrimitiveDef> {
    static_index().get(&id.0).map(|(_, def)| *def)
}

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
    for def in canonical_defs() {
        hash_def_identity(def, hasher);
    }
}

/// Hash one def's identity fields (see [`hash_prim_table_identity`]).
fn hash_def_identity<H: std::hash::Hasher>(def: &PrimitiveDef, hasher: &mut H) {
    use std::hash::Hash;
    def.name.hash(hasher);
    def.aliases.hash(hasher);
}

/// Register all primitive functions with the VM and build metadata.
pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for def in canonical_defs() {
        let doc = Doc::of(def);
        for name in std::iter::once(&def.name).chain(def.aliases) {
            meta.insert_def(symbols.intern(name), def);
            vm.docs.insert((*name).to_string(), doc.clone());
        }
    }

    super::docs::register_builtin_docs(&mut vm.docs);

    meta
}

/// Build primitive metadata without registering in a VM.
///
/// Walks the same canonical defs as `register_primitives` but records no
/// doc entries. Used by pipeline functions that receive an
/// already-configured VM.
pub fn build_primitive_meta(symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for def in canonical_defs() {
        for name in std::iter::once(&def.name).chain(def.aliases) {
            meta.insert_def(symbols.intern(name), def);
        }
    }

    meta
}

#[cfg(test)]
mod tests;
