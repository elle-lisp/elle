//! WASM backend: LIR → WASM emission and Wasmtime execution.
//!
//! Two modes:
//! - Full-module (`ELLE_WASM=1`): compiles stdlib + user code as one WASM
//!   module, replaces the bytecode VM entirely.
//! - Tiered (`ELLE_WASM_TIER=1`): compiles individual hot closures to WASM
//!   on demand, complements the bytecode VM.
//!
//! Architecture:
//! - `emit` — Module structure, WasmEmitter state, orchestration
//! - `instruction` — LIR instruction → WASM instruction translation
//! - `controlflow` — CFG emission, loop+br_table dispatch, terminators
//! - `suspend` — CPS suspension/resume, spill/restore, block splitting
//! - `handle` — Handle table mapping u64 handles to `Value`
//! - `host` — Host state (`ElleHost`), primitive dispatch, I/O
//! - `linker` — Wasmtime host function registration
//! - `store` — Engine/Store setup, env preparation, module execution
//! - `resume` — Fiber resume chain for yield-through-call
//! - `regalloc` — Virtual register → WASM local compaction
//! - `lazy` — Tiered compilation (per-closure WASM in VM mode)
//!
//! Heap objects live on the host side behind opaque u64 handles.
//! WASM code passes handles to host functions for all heap operations.
//! Immediate values (int, float, nil, bool, symbol, keyword) are
//! constructed directly in WASM with no host call.

mod controlflow;
pub mod emit;
pub mod handle;
pub mod host;
mod instruction;
pub mod lazy;
pub mod linker;
pub mod regalloc;
pub mod resume;
pub mod store;
mod suspend;

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

    let full_source;
    let stdlib_form_count;
    let compile_source = if with_stdlib {
        // Count stdlib forms so epoch migration skips them.
        stdlib_form_count = crate::reader::read_syntax_all(STDLIB, "<stdlib>")
            .map(|s| s.len())
            .unwrap_or(0);
        // Splice include/include-file directives in user source BEFORE
        // wrapping in ev/run. The directives are top-level in user code
        // but would become nested (invisible) after the ev/run wrapper.
        let body_spliced = crate::pipeline::splice_includes(source, source_name)?;
        // Concatenate stdlib + user source wrapped in ev/run so the async
        // scheduler is active (needed for ev/spawn, fibers+I/O, TCP, etc.).
        // I/O inside fibers propagates SIG_IO to the scheduler; top-level
        // I/O executes inline via maybe_execute_io.
        // Epoch directives are hoisted before stdlib for extract_epoch.
        let (epoch_prefix, body) = if body_spliced.starts_with("(elle/epoch") {
            body_spliced.split_once('\n').unwrap_or((&body_spliced, ""))
        } else {
            ("", body_spliced.as_str())
        };
        // Wrap user code in its own thunk so the ev/run callback is small
        // and invariant. This keeps stdlib closure WASM bytes identical
        // across programs, enabling incremental compilation cache hits.
        // The user thunk is a separate WASM function that compiles fast.
        full_source = format!(
            "{}\n{}\n(ev/run (fn [] ((fn []\n{}\n))))",
            epoch_prefix, STDLIB, body
        );
        full_source.as_str()
    } else {
        stdlib_form_count = 0;
        source
    };

    // Compile source → LIR (file mode = letrec for mutual recursion)
    let t0 = std::time::Instant::now();
    let lir_func = crate::pipeline::compile_file_to_lir(
        compile_source,
        &mut symbols,
        source_name,
        stdlib_form_count,
    )?;
    let t1 = std::time::Instant::now();

    if crate::config::get().wasm_lir {
        eprintln!(
            "[lir] entry: regs={} locals={} blocks={}",
            lir_func.num_regs,
            lir_func.num_locals,
            lir_func.blocks.len()
        );
        for block in &lir_func.blocks {
            eprintln!("[lir]   {:?}:", block.label);
            for si in &block.instructions {
                eprintln!("[lir]     {:?}", si.instr);
            }
            eprintln!("[lir]     term: {:?}", block.terminator.terminator);
        }
    }

    // LIR → WASM bytes + constant pool
    let result = emit::emit_module(&lir_func);
    let t2 = std::time::Instant::now();

    // Dump WASM for analysis
    if crate::config::get().wasm_dump {
        std::fs::write("/tmp/elle-wasm-dump.wasm", &result.wasm_bytes).ok();
    }

    // Run on Wasmtime
    let engine = store::create_engine().map_err(|e| e.to_string())?;
    let mut wasm_store = store::create_store(&engine, result.const_pool, result.closure_bytecodes);
    let linker = linker::create_linker(&engine).map_err(|e| e.to_string())?;
    let t3 = std::time::Instant::now();

    // Module cache: hash the WASM bytes, check for a cached pre-compiled module.
    let module = if let Some(cache_dir) = &crate::config::get().cache {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        result.wasm_bytes.hash(&mut hasher);
        let hash = hasher.finish();
        let cache_path =
            std::path::PathBuf::from(&cache_dir).join(format!("module_{:016x}.bin", hash));

        if let Ok(bytes) = std::fs::read(&cache_path) {
            // SAFETY: we trust our own cache files.
            unsafe { wasmtime::Module::deserialize(&engine, &bytes) }
                .map_err(|e: wasmtime::Error| e.to_string())?
        } else {
            let module =
                store::compile_module(&engine, &result.wasm_bytes).map_err(|e| e.to_string())?;
            if let Ok(serialized) = module.serialize() {
                std::fs::create_dir_all(cache_dir).ok();
                store::atomic_write(&cache_path, &serialized);
            }
            module
        }
    } else {
        store::compile_module(&engine, &result.wasm_bytes).map_err(|e| e.to_string())?
    };
    let t4 = std::time::Instant::now();
    let ret = store::run_module(&linker, &mut wasm_store, &module).map_err(|e| e.to_string());
    let t5 = std::time::Instant::now();

    eprintln!("[wasm] funcs: {}  elle→LIR: {:.3}s  LIR→wasm: {:.3}s  wasmtime compile: {:.3}s  execute: {:.3}s  total: {:.3}s  wasm_bytes: {}",
        {
            fn count_nested(f: &crate::lir::LirFunction) -> usize {
                let mut n = 0;
                for block in &f.blocks {
                    for spanned in &block.instructions {
                        if let crate::lir::LirInstr::MakeClosure { func: nested, .. } = &spanned.instr {
                            n += 1 + count_nested(nested);
                        }
                    }
                }
                n
            }
            1 + count_nested(&lir_func)
        },
        (t1 - t0).as_secs_f64(),
        (t2 - t1).as_secs_f64(),
        (t4 - t3).as_secs_f64(),
        (t5 - t4).as_secs_f64(),
        (t5 - t0).as_secs_f64(),
        result.wasm_bytes.len());
    ret
}
