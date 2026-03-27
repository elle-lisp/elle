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

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../../stdlib.lisp");

/// Compile and execute Elle source through the WASM backend.
///
/// Full pipeline: source → reader → expander → analyzer → HIR → LIR → WASM → Wasmtime.
/// Used for testing and as the `ELLE_WASM=1` entry point.
pub fn eval_wasm(source: &str, source_name: &str) -> Result<Value, String> {
    eval_wasm_raw(source, source_name, false)
}

/// Compile and execute with stdlib prepended.
///
/// Stdlib closures are bytecode and can't be called from WASM, so we
/// compile stdlib + user source as a single unit. The implicit letrec
/// makes all stdlib definitions visible to user code.
pub fn eval_wasm_with_stdlib(source: &str, source_name: &str) -> Result<Value, String> {
    eval_wasm_raw(source, source_name, true)
}

fn eval_wasm_raw(source: &str, source_name: &str, with_stdlib: bool) -> Result<Value, String> {
    let mut vm = crate::vm::VM::new();
    let mut symbols = Box::new(crate::symbol::SymbolTable::new());
    crate::primitives::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut crate::symbol::SymbolTable = &mut *symbols;
    crate::context::set_symbol_table(sym_ptr);
    crate::primitives::set_length_symbol_table(sym_ptr);

    let full_source;
    let compile_source = if with_stdlib {
        // Wrap user source in parameterize for I/O backend, inside a
        // self-calling lambda to create a function scope.  Without the
        // lambda, top-level `def` forms in user code leak into the
        // file-level letrec scope and corrupt Pass 3 fixpoint
        // re-analysis of stdlib lambdas.
        full_source = format!(
            "{}\n(parameterize [(*io-backend* (io/backend :async))]\n((fn ()\n{}\n)))",
            STDLIB, source
        );
        full_source.as_str()
    } else {
        source
    };

    // Compile source → LIR (file mode = letrec for mutual recursion)
    let lir_func = crate::pipeline::compile_file_to_lir(compile_source, &mut symbols, source_name)?;

    // LIR → WASM bytes + constant pool
    let result = emit::emit_module(&lir_func);

    // Run on Wasmtime
    let engine = store::create_engine().map_err(|e| e.to_string())?;
    let mut wasm_store = store::create_store(&engine, result.const_pool);
    let linker = store::create_linker(&engine).map_err(|e| e.to_string())?;
    let module = store::compile_module(&engine, &result.wasm_bytes).map_err(|e| e.to_string())?;
    store::run_module(&linker, &mut wasm_store, &module).map_err(|e| e.to_string())
}
