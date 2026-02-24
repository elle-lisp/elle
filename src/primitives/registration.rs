use crate::effects::Effect;
use crate::symbol::SymbolTable;
use crate::value::types::Arity;
use crate::value::Value;
use crate::vm::VM;

use super::def::{PrimitiveDef, PrimitiveDoc, PrimitiveMeta};
use super::{
    arithmetic, array, bitwise, cell, comparison, concurrency, coroutines, debug, debugging,
    display, fibers, file_io, json, list, logic, math, meta, module_loading, package, process,
    string, structs, table, time, type_check,
};

/// FFI primitives — defined here because the ffi module doesn't own
/// a PRIMITIVES table (it lives outside `primitives/`).
const FFI_PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "load-library",
        func: crate::ffi_primitives::prim_load_library_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Load a shared library",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "list-libraries",
        func: crate::ffi_primitives::prim_list_libraries_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(0),
        doc: "List loaded libraries",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "call-c-function",
        func: crate::ffi_primitives::prim_call_c_function_wrapper,
        effect: Effect::raises(),
        arity: Arity::AtLeast(3),
        doc: "Call a C function",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "load-header-with-lib",
        func: crate::ffi_primitives::prim_load_header_with_lib_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Load a C header with library",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "define-enum",
        func: crate::ffi_primitives::prim_define_enum_wrapper,
        effect: Effect::raises(),
        arity: Arity::AtLeast(1),
        doc: "Define a C enum",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "make-c-callback",
        func: crate::ffi_primitives::prim_make_c_callback_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(3),
        doc: "Create a C callback from an Elle function",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "free-callback",
        func: crate::ffi_primitives::prim_free_callback_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Free a C callback",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "register-allocation",
        func: crate::ffi_primitives::prim_register_allocation_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(0),
        doc: "Register an allocation",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "memory-stats",
        func: crate::ffi_primitives::prim_memory_stats_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(0),
        doc: "Get memory statistics",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "type-check",
        func: crate::ffi_primitives::prim_type_check_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Check a value's C type",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "null-pointer?",
        func: crate::ffi_primitives::prim_null_pointer_wrapper,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if a pointer is null",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "ffi-last-error",
        func: crate::ffi_primitives::prim_ffi_last_error_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(0),
        doc: "Get the last FFI error",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
    PrimitiveDef {
        name: "with-ffi-safety-checks",
        func: crate::ffi_primitives::prim_with_ffi_safety_checks_wrapper,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Run with FFI safety checks enabled",
        category: "ffi",
        ..PrimitiveDef::DEFAULT
    },
];

/// All primitive tables. Each module exports a `const PRIMITIVES`
/// array; this list is the single place that enumerates them.
const ALL_TABLES: &[&[PrimitiveDef]] = &[
    arithmetic::PRIMITIVES,
    array::PRIMITIVES,
    bitwise::PRIMITIVES,
    cell::PRIMITIVES,
    comparison::PRIMITIVES,
    concurrency::PRIMITIVES,
    coroutines::PRIMITIVES,
    debug::PRIMITIVES,
    debugging::PRIMITIVES,
    display::PRIMITIVES,
    fibers::PRIMITIVES,
    file_io::PRIMITIVES,
    json::PRIMITIVES,
    list::PRIMITIVES,
    logic::PRIMITIVES,
    math::PRIMITIVES,
    meta::PRIMITIVES,
    module_loading::PRIMITIVES,
    package::PRIMITIVES,
    process::PRIMITIVES,
    string::PRIMITIVES,
    structs::PRIMITIVES,
    table::PRIMITIVES,
    time::PRIMITIVES,
    type_check::PRIMITIVES,
    FFI_PRIMITIVES,
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

            let doc = PrimitiveDoc {
                name: def.name,
                doc: def.doc,
                params: def.params,
                arity: def.arity,
                effect: def.effect,
                category: def.category,
                example: def.example,
            };
            vm.primitive_docs.insert(def.name.to_string(), doc.clone());

            for alias in def.aliases {
                let alias_id = symbols.intern(alias);
                vm.set_global(alias_id.0, Value::native_fn(def.func));
                meta.effects.insert(alias_id, def.effect);
                meta.arities.insert(alias_id, def.arity);
                vm.primitive_docs.insert((*alias).to_string(), doc.clone());
            }
        }
    }

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
