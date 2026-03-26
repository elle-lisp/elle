//! WASM backend: LIR → WASM emission and Wasmtime execution.
//!
//! Replaces the bytecode emitter (`lir/emit/`) and VM dispatch loop (`vm/`)
//! with WASM module generation and Wasmtime execution.
//!
//! Architecture:
//! - `emit` — LIR → WASM module bytes (via `wasm-encoder`)
//! - `handle` — Handle table mapping u64 → Rc<HeapObject>
//! - `host` — Wasmtime host functions (primitive dispatch, runtime support)
//! - `store` — Engine/Store/Linker management
//!
//! Heap objects live on the host side behind opaque u64 handles.
//! WASM code passes handles to host functions for all heap operations.
//! Immediate values (int, float, nil, bool, symbol, keyword) are
//! constructed directly in WASM with no host call.

pub mod emit;
pub mod handle;
pub mod host;
pub mod store;

use crate::value::Value;

/// Compile and execute Elle source through the WASM backend.
///
/// Full pipeline: source → reader → expander → analyzer → HIR → LIR → WASM → Wasmtime.
/// Used for testing and as the `ELLE_WASM=1` entry point.
pub fn eval_wasm(source: &str, source_name: &str) -> Result<Value, String> {
    let mut vm = crate::vm::VM::new();
    let mut symbols = Box::new(crate::symbol::SymbolTable::new());
    crate::primitives::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut crate::symbol::SymbolTable = &mut *symbols;
    crate::context::set_symbol_table(sym_ptr);
    crate::primitives::set_length_symbol_table(sym_ptr);

    // Compile source → LIR (file mode = letrec for mutual recursion)
    let lir_func = crate::pipeline::compile_file_to_lir(source, &mut symbols, source_name)?;

    // LIR → WASM bytes + constant pool
    let result = emit::emit_module(&lir_func);

    // Run on Wasmtime
    let engine = store::create_engine().map_err(|e| e.to_string())?;
    let mut wasm_store = store::create_store(&engine, result.const_pool);
    let linker = store::create_linker(&engine).map_err(|e| e.to_string())?;
    let module = store::compile_module(&engine, &result.wasm_bytes).map_err(|e| e.to_string())?;
    store::run_module(&linker, &mut wasm_store, &module).map_err(|e| e.to_string())
}
