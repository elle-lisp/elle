//! WASM backend: LIR → WASM emission and Wasmtime execution.
//!
//! Two modes:
//! - Full-module: compiles stdlib + user code as one WASM
//!   module, replaces the bytecode VM entirely.
//! - Tiered: compiles individual hot closures to WASM
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
mod liveness;
pub mod linker;
pub mod regalloc;
pub mod resume;
pub mod store;
mod suspend;

use crate::value::Value;

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../../stdlib.lisp");

/// Maximum number of top-level forms per user-code thunk.
/// Balances WASM function size (Wasmtime compile time) against
/// thunk-call overhead. 25 forms keeps each chunk under ~200KB of
/// WASM text while allowing Wasmtime to parallelize compilation.
const CHUNK_SIZE: usize = 25;

/// Split user source forms into thunks for parallel WASM compilation.
///
/// Forms that define bindings (def, defn, var, defmacro, signal) stay
/// at the top level so they're visible to subsequent chunks.
/// Expression forms (asserts, bare calls) are grouped into chunks,
/// each wrapped in `((fn [] ...))`. The last chunk's return value
/// is the overall return value.
///
/// If the source has few forms or is unparseable, returns it unchanged
/// wrapped in a single thunk.
fn chunk_user_forms(source: &str, source_name: &str) -> String {
    let forms = match crate::reader::read_syntax_all(source, source_name) {
        Ok(f) => f,
        Err(_) => return format!("((fn []\n{}\n))", source),
    };

    // Classify forms: definitions stay top-level, expressions get chunked.
    // Track byte ranges so we can slice the original source.
    let mut parts: Vec<(bool, usize, usize)> = Vec::new(); // (is_def, start, end)
    for form in &forms {
        let is_def = form
            .as_list()
            .and_then(|l| l.first())
            .and_then(|s| s.as_symbol())
            .is_some_and(|s| {
                // Conservative: treat any form that might define a binding
                // as a definition. This includes macros like ffi/defbind
                // that expand to (def ...). Better to under-chunk than to
                // break scoping.
                s.starts_with("def")
                    || s.starts_with("var")
                    || s == "signal"
                    || s.starts_with("include")
                    || s.contains("/def")
            });
        parts.push((is_def, form.span.start, form.span.end));
    }

    // Count expression forms
    let expr_count = parts.iter().filter(|(is_def, _, _)| !is_def).count();
    if expr_count <= CHUNK_SIZE {
        // Small enough — single thunk, no chunking needed.
        return format!("((fn []\n{}\n))", source);
    }

    // Build output: defs at top level, expressions in chunked thunks.
    let mut output = String::new();
    let mut expr_chunk: Vec<&str> = Vec::new();

    for (is_def, start, end) in &parts {
        let slice = &source[*start..*end];
        if *is_def {
            // Flush pending expression chunk before the def
            if !expr_chunk.is_empty() {
                output.push_str("((fn []\n");
                for e in &expr_chunk {
                    output.push_str(e);
                    output.push('\n');
                }
                output.push_str("))\n");
                expr_chunk.clear();
            }
            output.push_str(slice);
            output.push('\n');
        } else {
            expr_chunk.push(slice);
            if expr_chunk.len() >= CHUNK_SIZE {
                output.push_str("((fn []\n");
                for e in &expr_chunk {
                    output.push_str(e);
                    output.push('\n');
                }
                output.push_str("))\n");
                expr_chunk.clear();
            }
        }
    }

    // Flush remaining expressions
    if !expr_chunk.is_empty() {
        output.push_str("((fn []\n");
        for e in &expr_chunk {
            output.push_str(e);
            output.push('\n');
        }
        output.push_str("))\n");
    }

    output
}

/// Compile and execute Elle source through the WASM backend.
///
/// Full pipeline: source → reader → expander → analyzer → HIR → LIR → WASM → Wasmtime.
/// Used for testing and as the full-module WASM entry point.
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

/// Compile a WASM module, checking the disk cache first.
///
/// Returns a compiled Module. On cache miss, compiles from bytes,
/// serializes, and caches atomically.
fn compile_or_cache_module(
    engine: &wasmtime::Engine,
    wasm_bytes: &[u8],
) -> Result<wasmtime::Module, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    if let Some(cache_dir) = &crate::config::get().cache {
        let mut hasher = DefaultHasher::new();
        wasm_bytes.hash(&mut hasher);
        let hash = hasher.finish();
        let cache_path =
            std::path::PathBuf::from(cache_dir).join(format!("closure_{:016x}.bin", hash));

        if let Ok(bytes) = std::fs::read(&cache_path) {
            return unsafe { wasmtime::Module::deserialize(engine, &bytes) }
                .map_err(|e| e.to_string());
        }

        let module = wasmtime::Module::new(engine, wasm_bytes).map_err(|e| e.to_string())?;
        if let Ok(serialized) = module.serialize() {
            std::fs::create_dir_all(cache_dir).ok();
            store::atomic_write(&cache_path, &serialized);
        }
        Ok(module)
    } else {
        wasmtime::Module::new(engine, wasm_bytes).map_err(|e| e.to_string())
    }
}

fn eval_wasm_raw(source: &str, source_name: &str, with_stdlib: bool) -> Result<Value, String> {
    let mut vm = crate::vm::VM::new();
    let mut symbols = Box::new(crate::symbol::SymbolTable::new());
    crate::primitives::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut crate::symbol::SymbolTable = &mut *symbols;
    crate::context::set_symbol_table(sym_ptr);

    let full_source;
    let mut stdlib_form_count;
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
        // Strip stdlib's own epoch tag to avoid duplicates.
        let (epoch_prefix, body) = if body_spliced.starts_with("(elle/epoch") {
            body_spliced.split_once('\n').unwrap_or((&body_spliced, ""))
        } else {
            ("", body_spliced.as_str())
        };
        let epoch_tag = format!("(elle/epoch {})", crate::epoch::CURRENT_EPOCH);
        let (stdlib_body, stripped_epoch) = STDLIB
            .strip_prefix(&format!("{}\n", epoch_tag))
            .or_else(|| STDLIB.strip_prefix(&format!("{}\r\n", epoch_tag)))
            .map(|s| (s, true))
            .unwrap_or((STDLIB, false));
        if stripped_epoch {
            stdlib_form_count = stdlib_form_count.saturating_sub(1);
        }
        let wrapped_body = if crate::config::get().wasm_chunk {
            chunk_user_forms(body, source_name)
        } else {
            format!("((fn []\n{}\n))", body)
        };
        full_source = format!(
            "{}\n{}\n(ev/run (fn []\n{}\n))",
            epoch_prefix, stdlib_body, wrapped_body
        );
        full_source.as_str()
    } else {
        stdlib_form_count = 0;
        source
    };

    // Compile source → LIR (file mode = letrec for mutual recursion)
    let t0 = std::time::Instant::now();
    let lir_module = crate::pipeline::compile_file_to_lir(
        compile_source,
        &mut symbols,
        source_name,
        stdlib_form_count,
    )?;
    let t1 = std::time::Instant::now();

    if crate::config::get().wasm_lir {
        eprintln!(
            "[lir] entry: regs={} locals={} blocks={} closures={}",
            lir_module.entry.num_regs,
            lir_module.entry.num_locals,
            lir_module.entry.blocks.len(),
            lir_module.closures.len(),
        );
        for block in &lir_module.entry.blocks {
            eprintln!("[lir]   {:?}:", block.label);
            for si in &block.instructions {
                eprintln!("[lir]     {:?}", si.instr);
            }
            eprintln!("[lir]     term: {:?}", block.terminator.terminator);
        }
    }

    // Per-closure pre-compilation: compile each closure as a standalone
    // Module, cached by WASM bytes hash. The full module gets stubs for
    // pre-compiled closures (tiny, compile instantly). At runtime, rt_call
    // dispatches to pre-compiled Modules instead of the full module's table.
    let engine = store::create_engine().map_err(|e| e.to_string())?;
    let mut precached: Vec<Option<host::PrecachedClosure>> = vec![None; lir_module.closures.len()];
    let mut stubbed = std::collections::HashSet::new();

    if crate::config::get().cache.is_some() {
        for (i, closure_func) in lir_module.closures.iter().enumerate() {
            if let Some(standalone) = emit::emit_single_closure(closure_func, Some(&lir_module)) {
                if let Ok(module) = compile_or_cache_module(&engine, &standalone.wasm_bytes) {
                    precached[i] = Some(host::PrecachedClosure {
                        module,
                        const_pool: standalone.const_pool,
                    });
                    stubbed.insert(crate::lir::ClosureId(i as u32));
                }
            }
        }
    }

    // LIR → WASM bytes + constant pool. Stubbed closures get minimal
    // bodies (unreachable) since they're served by pre-compiled Modules.
    let result = emit::emit_module(&lir_module, stubbed);
    let t2 = std::time::Instant::now();

    // Dump WASM for analysis
    if crate::config::get().wasm_dump {
        std::fs::write("/tmp/elle-wasm-dump.wasm", &result.wasm_bytes).ok();
    }

    let mut wasm_store = store::create_store(&engine, result.const_pool, result.closure_bytecodes);
    wasm_store.data_mut().precached_closures = precached;
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

    let funcs = 1 + lir_module.closures.len();
    let lir_secs = (t1 - t0).as_secs_f64();
    let emit_secs = (t2 - t1).as_secs_f64();
    let compile_secs = (t4 - t3).as_secs_f64();
    let exec_secs = (t5 - t4).as_secs_f64();
    let total_secs = (t5 - t0).as_secs_f64();
    let wasm_bytes = result.wasm_bytes.len();

    if crate::config::get().json {
        eprintln!(
            "{}",
            serde_json::json!({
                "wasm": {
                    "funcs": funcs,
                    "lir_secs": lir_secs,
                    "emit_secs": emit_secs,
                    "compile_secs": compile_secs,
                    "exec_secs": exec_secs,
                    "total_secs": total_secs,
                    "wasm_bytes": wasm_bytes,
                }
            })
        );
    } else {
        eprintln!("[wasm] funcs: {}  elle→LIR: {:.3}s  LIR→wasm: {:.3}s  wasmtime compile: {:.3}s  execute: {:.3}s  total: {:.3}s  wasm_bytes: {}",
            funcs, lir_secs, emit_secs, compile_secs, exec_secs, total_secs, wasm_bytes);
    }
    ret
}
