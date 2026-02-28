use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

use super::def::{Doc, PrimitiveDef, PrimitiveMeta};
use super::{
    arithmetic, array, bitwise, buffer, bytes, cell, comparison, concurrency, convert, coroutines,
    crypto, debug, debugging, display, ffi, fibers, file_io, json, list, logic, math, meta,
    module_loading, package, path, process, read, string, structs, table, time, type_check,
};

/// All primitive tables. Each module exports a `const PRIMITIVES`
/// array; this list is the single place that enumerates them.
pub(crate) const ALL_TABLES: &[&[PrimitiveDef]] = &[
    arithmetic::PRIMITIVES,
    array::PRIMITIVES,
    bitwise::PRIMITIVES,
    buffer::PRIMITIVES,
    bytes::PRIMITIVES,
    cell::PRIMITIVES,
    comparison::PRIMITIVES,
    convert::PRIMITIVES,
    concurrency::PRIMITIVES,
    coroutines::PRIMITIVES,
    crypto::PRIMITIVES,
    debug::PRIMITIVES,
    debugging::PRIMITIVES,
    display::PRIMITIVES,
    ffi::PRIMITIVES,
    fibers::PRIMITIVES,
    file_io::PRIMITIVES,
    json::PRIMITIVES,
    list::PRIMITIVES,
    logic::PRIMITIVES,
    math::PRIMITIVES,
    meta::PRIMITIVES,
    module_loading::PRIMITIVES,
    package::PRIMITIVES,
    path::PRIMITIVES,
    process::PRIMITIVES,
    read::PRIMITIVES,
    string::PRIMITIVES,
    structs::PRIMITIVES,
    table::PRIMITIVES,
    time::PRIMITIVES,
    type_check::PRIMITIVES,
];

/// Register all primitive functions with the VM and build metadata.
pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for table in ALL_TABLES {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            vm.set_global(sym_id.0, Value::native_fn(def.func));
            meta.effects.insert(sym_id, def.effect);
            meta.arities.insert(sym_id, def.arity);

            let doc = Doc {
                name: def.name,
                doc: def.doc,
                params: def.params,
                arity: def.arity,
                effect: def.effect,
                category: def.category,
                example: def.example,
                aliases: def.aliases,
            };
            vm.docs.insert(def.name.to_string(), doc.clone());

            for alias in def.aliases {
                let alias_id = symbols.intern(alias);
                vm.set_global(alias_id.0, Value::native_fn(def.func));
                meta.effects.insert(alias_id, def.effect);
                meta.arities.insert(alias_id, def.arity);
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
/// only builds the effects/arities maps. Used by pipeline functions
/// that receive an already-configured VM.
pub fn build_primitive_meta(symbols: &mut SymbolTable) -> PrimitiveMeta {
    let mut meta = PrimitiveMeta::new();

    for table in ALL_TABLES {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            meta.effects.insert(sym_id, def.effect);
            meta.arities.insert(sym_id, def.arity);

            for alias in def.aliases {
                let alias_id = symbols.intern(alias);
                meta.effects.insert(alias_id, def.effect);
                meta.arities.insert(alias_id, def.arity);
            }
        }
    }

    meta
}
