use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

use super::def::{Doc, PrimitiveDef, PrimitiveMeta};
use super::{
    allocator, arena, arithmetic, array, bitwise, bytes, calling, cell, chan, comparison, compile,
    concurrency, convert, coroutines, debug, disassembly, display, fiber_introspect, fibers,
    fileio, format, introspection, io, json, list, loading, logic, lstruct, math, memory, meta,
    modules, net, package, parameters, path, ports, read, sets, sort, stream, string, structs,
    subprocess, time, traits, types, unix, watch,
};

/// All primitive tables. Each module exports a `const PRIMITIVES`
/// array; this list is the single place that enumerates them.
pub(crate) const ALL_TABLES: &[&[PrimitiveDef]] = &[
    allocator::PRIMITIVES,
    arena::PRIMITIVES,
    arithmetic::PRIMITIVES,
    array::PRIMITIVES,
    bitwise::PRIMITIVES,
    bytes::PRIMITIVES,
    calling::PRIMITIVES,
    cell::PRIMITIVES,
    chan::PRIMITIVES,
    compile::PRIMITIVES,
    comparison::PRIMITIVES,
    convert::PRIMITIVES,
    concurrency::PRIMITIVES,
    coroutines::PRIMITIVES,
    debug::PRIMITIVES,
    disassembly::PRIMITIVES,
    display::PRIMITIVES,
    fiber_introspect::PRIMITIVES,
    fibers::PRIMITIVES,
    fileio::PRIMITIVES,
    format::PRIMITIVES,
    introspection::PRIMITIVES,
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

/// Register all primitive functions with the VM and build metadata.
pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for table in ALL_TABLES {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            let native_val = Value::native_fn(def.func);
            meta.signals.insert(sym_id, def.signal);
            meta.arities.insert(sym_id, def.arity);
            meta.functions.insert(sym_id, native_val);

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
                let alias_val = Value::native_fn(def.func);
                meta.signals.insert(alias_id, def.signal);
                meta.arities.insert(alias_id, def.arity);
                meta.functions.insert(alias_id, alias_val);
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

    for table in ALL_TABLES {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            meta.signals.insert(sym_id, def.signal);
            meta.arities.insert(sym_id, def.arity);
            meta.functions.insert(sym_id, Value::native_fn(def.func));

            for alias in def.aliases {
                let alias_id = symbols.intern(alias);
                meta.signals.insert(alias_id, def.signal);
                meta.arities.insert(alias_id, def.arity);
                meta.functions.insert(alias_id, Value::native_fn(def.func));
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
    for table in ALL_TABLES {
        for def in *table {
            symbols.intern(def.name);
            for alias in def.aliases {
                symbols.intern(alias);
            }
        }
    }
}

use std::cell::RefCell;

thread_local! {
    static PRIMITIVE_META_CACHE: RefCell<Option<PrimitiveMeta>> = const { RefCell::new(None) };
}

/// Return cached primitive metadata, building it on first call.
///
/// The cache is never invalidated — primitive metadata is immutable
/// within a process lifetime. Callers must have already interned
/// primitives in their SymbolTable (the cache skips the intern
/// side-effect on hit).
pub fn cached_primitive_meta(symbols: &mut SymbolTable) -> PrimitiveMeta {
    PRIMITIVE_META_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        match &*cache_ref {
            Some(meta) => meta.clone(),
            None => {
                let meta = build_primitive_meta(symbols);
                cache_ref.replace(meta.clone());
                meta
            }
        }
    })
}

/// Add stdlib exports to the cached PrimitiveMeta.
///
/// Called by `init_stdlib` after stdlib execution. Updates the
/// PRIMITIVE_META_CACHE so that `cached_primitive_meta` returns
/// metadata including stdlib exports.
pub(crate) fn update_primitive_meta_cache(
    exports: &std::collections::HashMap<
        crate::value::SymbolId,
        (crate::value::Value, crate::signals::Signal),
    >,
) {
    PRIMITIVE_META_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let meta = cache_ref.get_or_insert_with(PrimitiveMeta::default);
        for (sym_id, (value, signal)) in exports {
            meta.signals.insert(*sym_id, *signal);
            meta.functions.insert(*sym_id, *value);
        }
    });
}
