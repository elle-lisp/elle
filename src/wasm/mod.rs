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
pub mod linker;
mod liveness;
mod outcome;
pub mod regalloc;
pub mod resume;
pub mod store;
mod suspend;

#[cfg(test)]
mod tests;

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../stdlib.lisp");

/// Where `--wasm-dump` writes the emitted module bytes. Lives on the /dev/shm
/// tmpfs, not /tmp: /tmp is a shared, size-limited filesystem that other
/// services rely on, so debug artifacts belong on the throwaway tmpfs.
const WASM_DUMP_PATH: &str = "/dev/shm/elle-wasm-dump.wasm";

/// Whether the user source defines the same top-level name twice — a
/// redefinition the naive single-thunk wrap would reject as a duplicate binding.
///
/// Conservative and name-based: for each top-level `(def* …)` / `(var …)` /
/// `*/def*` form it collects the leaf symbols of the binding target — a bare
/// name (`(def x …)`) or every name a destructuring pattern binds
/// (`(def [ok _] …)` binds `ok`, the `_` wildcard is ignored). A symbol seen
/// twice selects the top-level restructure (build_scheduled_toplevel); otherwise
/// the single-thunk wrap is kept, which preserves closure execution context for
/// the whole program. A redefinition introduced only by macro expansion is not
/// visible here and would fall through to the single wrap — none exist in the
/// corpus, whose redefinitions are all source-level defs.
fn has_toplevel_redefinition(source: &str, source_name: &str) -> bool {
    // Source in, a yes/no out: the tree gets its own heap and dies with it.
    let mut home = crate::syntax::SyntaxHeap::new();
    let forms = match crate::reader::read_syntax_all(home.arena(), source, source_name) {
        Ok(f) => f,
        Err(_) => return false,
    };
    // Collect the leaf symbols a def binding target introduces (skipping `_`).
    fn collect_names(target: &crate::syntax::Syntax, out: &mut Vec<String>) {
        if let Some(name) = target.as_symbol() {
            if name != "_" {
                out.push(name.to_string());
            }
        } else if let Some(items) = target.as_list_or_tuple() {
            for item in items {
                collect_names(item, out);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for form in &forms {
        let Some(list) = form.as_list() else { continue };
        let head = list.first().and_then(|s| s.as_symbol());
        let is_def = head
            .is_some_and(|s| s.starts_with("def") || s.starts_with("var") || s.contains("/def"));
        if !is_def {
            continue;
        }
        let Some(target) = list.get(1) else { continue };
        let mut names = Vec::new();
        collect_names(target, &mut names);
        for name in names {
            if !seen.insert(name) {
                return true;
            }
        }
    }
    false
}

/// Rewrite user source so its top-level DEFINITIONS stay at the file-letrec top
/// level while each run of consecutive EXPRESSIONS is wrapped in
/// `(ev/run (fn [] …))` to run under the async scheduler.
///
/// Two properties this buys, together:
/// - **Top-level def semantics.** A file's top level uses sequential shadowing,
///   so `(def a 10) (def a (+ a 1))` is a redefinition. Keeping defs out of any
///   `(fn [] …)` preserves that; nesting them in a thunk makes the body a
///   fn-body letrec* where a duplicate binding is an error (the `--wasm=full`
///   divergence from the VM this rewrite removes).
/// - **Scheduler.** Expressions still run inside `ev/run`, so `ev/spawn`,
///   fibers+I/O, and `sys/join`'s scheduler-cooperative deadline work.
///
/// Order is preserved: the pending expression run is flushed (emitted as one
/// `ev/run`) before each def, so a spawn and its join in the same expression run
/// share one scheduler session. Definitions are detected conservatively (any
/// `def*`/`var`/`signal`/`include`/`*/def*` head), so a macro that expands to a
/// `def` is treated as one — better to leave a form at top level than to nest a
/// binding wrongly. Unparseable source falls back to a single scheduled thunk.
fn build_scheduled_toplevel(source: &str, source_name: &str) -> String {
    // Source text in, source text out: the tree only picks the split points,
    // so it gets its own heap and dies with it.
    let mut home = crate::syntax::SyntaxHeap::new();
    let forms = match crate::reader::read_syntax_all(home.arena(), source, source_name) {
        Ok(f) => f,
        Err(_) => return format!("(ev/run (fn []\n{}\n))", source),
    };

    let is_def = |form: &crate::syntax::Syntax| {
        form.as_list()
            .and_then(|l| l.first())
            .and_then(|s| s.as_symbol())
            .is_some_and(|s| {
                s.starts_with("def")
                    || s.starts_with("var")
                    || s == "signal"
                    || s.starts_with("include")
                    || s.contains("/def")
            })
    };

    let mut output = String::new();
    let mut expr_run: Vec<&str> = Vec::new();
    let flush = |output: &mut String, run: &mut Vec<&str>| {
        if run.is_empty() {
            return;
        }
        output.push_str("(ev/run (fn []\n");
        for e in run.iter() {
            output.push_str(e);
            output.push('\n');
        }
        output.push_str("))\n");
        run.clear();
    };

    for form in &forms {
        let slice = &source[form.span.start as usize..form.span.end as usize];
        if is_def(form) {
            flush(&mut output, &mut expr_run);
            output.push_str(slice);
            output.push('\n');
        } else {
            expr_run.push(slice);
        }
    }
    flush(&mut output, &mut expr_run);
    output
}

/// Compile and execute Elle source through the WASM backend.
///
/// Full pipeline: source → reader → expander → analyzer → HIR → LIR → WASM → Wasmtime.
/// Used for testing and as the full-module WASM entry point. Returns the result's
/// display form (materialized while the backend heap is alive — a heap-valued
/// result would dangle once that heap drops on return).
pub fn eval_wasm(source: &str, source_name: &str) -> Result<String, String> {
    eval_wasm_raw(source, source_name, false)
}

/// Compile and execute with stdlib prepended.
///
/// Stdlib closures are bytecode and can't be called from WASM, so we
/// compile stdlib + user source as a single unit. The implicit letrec
/// makes all stdlib definitions visible to user code. Returns the result's
/// display form (see [`eval_wasm`]).
pub fn eval_wasm_with_stdlib(source: &str, source_name: &str) -> Result<String, String> {
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
    let cache_path = store::cache_path_for("closure", wasm_bytes);
    store::cached_or_compile(engine, wasm_bytes, cache_path.as_deref()).map_err(|e| e.to_string())
}

/// Build the source the full-module WASM path actually compiles: the stdlib
/// concatenated with the user code, the latter rewritten by
/// [`build_scheduled_toplevel`] so definitions stay top-level and expression
/// runs execute under `ev/run`.
///
/// Returns `(full_source, stdlib_form_count)`. `stdlib_form_count` is the number
/// of leading forms epoch migration must skip (the stdlib forms are already in
/// the current epoch); `compile_file_to_lir` migrates only the user forms after
/// them.
///
/// Extracted from `eval_wasm_raw` so the exact spliced source is reachable from
/// tests that need to inspect the compiled LIR.
fn build_full_source(source: &str, source_name: &str) -> Result<(String, usize), String> {
    // Count stdlib forms so epoch migration skips them.
    let mut home = crate::syntax::SyntaxHeap::new();
    let mut stdlib_form_count = crate::reader::read_syntax_all(home.arena(), STDLIB, "<stdlib>")
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
    // Strip any (elle/epoch N) from stdlib, not just the current epoch.
    // Avoids a footgun where bumping CURRENT_EPOCH breaks --wasm=full.
    let (stdlib_body, stripped_epoch) = if STDLIB.starts_with("(elle/epoch") {
        let rest = STDLIB.split_once('\n').map(|(_, r)| r).unwrap_or("");
        (rest, true)
    } else {
        (STDLIB, false)
    };
    if stripped_epoch {
        stdlib_form_count = stdlib_form_count.saturating_sub(1);
    }
    // User DEFINITIONS stay at the file-letrec top level; only consecutive
    // EXPRESSION runs are wrapped in `(ev/run (fn [] …))`. This matches the VM,
    // whose `execute_scheduled` wraps the scheduler around already-top-level-
    // analyzed bytecode (src/vm/mod.rs): a file's top level uses sequential
    // shadowing, so `(def a 10) (def a (+ a 1))` is a redefinition, not an error.
    // Nesting the whole body in a single `(fn [] …)` instead (the naive wrap)
    // makes those defs a fn-body letrec* where a duplicate binding is rejected —
    // the divergence that failed def-shadow/numeric/… under `--wasm=full`
    // (src/wasm/tests.rs `wasm_full_allows_toplevel_def_redefinition`).
    //
    // But the restructure has a cost: a top-level def's RHS then runs in the
    // ENTRY function, and some operations (`eval`'s dynamic compilation) trap
    // there while working in a closure. The single-thunk wrap keeps the WHOLE
    // program in a closure, so it is the safe default; the restructure is used
    // ONLY when the program actually redefines a top-level name — the case the
    // single wrap cannot compile. (A file that both redefines AND calls `eval`
    // in a def RHS would still hit the entry-`eval` limitation, but none do; the
    // corpus's redefining files keep `eval` inside expressions.) Pinned by
    // `wasm_full_allows_toplevel_def_redefinition` (restructure path) and
    // tests/elle/region-termination-sweep.lisp (single-wrap `eval`-in-def path).
    let scheduled_body = if has_toplevel_redefinition(body, source_name) {
        build_scheduled_toplevel(body, source_name)
    } else {
        format!("(ev/run (fn []\n((fn []\n{}\n))\n))", body)
    };
    let full_source = format!("{}\n{}\n{}", epoch_prefix, stdlib_body, scheduled_body);
    Ok((full_source, stdlib_form_count))
}

fn eval_wasm_raw(source: &str, source_name: &str, with_stdlib: bool) -> Result<String, String> {
    // One heap shared by the program/eval VM and the compile context's
    // macro-expansion VM (as `RuntimeCore::bare` does). Compile-time macro
    // expansion runs on the compile context's macro VM; stdlib closures it must
    // call (see the `init_stdlib` note below) are created on this heap by the
    // program VM, so the two VMs must share it for the cross-VM call to resolve.
    let mut heap = Box::new(crate::value::fiberheap::FiberHeap::new());
    let heap_ptr: *mut crate::value::fiberheap::FiberHeap = &mut *heap;
    let mut vm = crate::vm::VM::new_with_heap(heap_ptr);
    let mut symbols = Box::new(crate::symbol::SymbolTable::new());
    crate::primitives::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut crate::symbol::SymbolTable = &mut *symbols;
    // Point the VM at this instance's symbol table (stable boxed address).
    vm.set_symbols(sym_ptr);
    // This standalone eval owns its per-instance compile context (the stdlib it
    // needs is spliced into the source below, so it accumulates during compile);
    // wire the VM to it so a runtime `(eval …)` in the program resolves here.
    let mut compile = Box::new(crate::pipeline::CompileCtx::new_with_heap(heap_ptr));
    vm.set_compile_ctx(&mut *compile as *mut crate::pipeline::CompileCtx);

    // Load stdlib into the compile context so COMPILE-TIME macro expansion
    // resolves stdlib functions. A prelude or user macro's transformer body may
    // call a stdlib `defn` while expanding — `assert`'s transformer calls
    // `pair?` (src/stdlib.lisp) to detect a comparison form — and expansion runs
    // on the macro VM, not in the spliced source. Without stdlib loaded here that
    // call is unbound and expansion fails with "undefined variable: pair?" before
    // any WASM is emitted (src/wasm/tests.rs `wasm_full_expands_assert_macro`).
    // This is independent of the source-splice in `build_full_source`: the splice
    // makes stdlib callable from WASM at RUNTIME; this load makes it callable
    // during macro expansion at COMPILE time. User references still bind to the
    // spliced letrec definitions (lexical scope shadows the registered exports),
    // so the emitted WASM calls the compiled stdlib, not the macro-VM closures.
    if with_stdlib {
        crate::primitives::init_stdlib(
            &mut vm,
            &mut symbols,
            &mut compile,
            &crate::compiler::stdlib_cache::StdlibCache::Process,
        );
    }

    let full_source;
    let stdlib_form_count;
    let compile_source = if with_stdlib {
        let (fs, count) = build_full_source(source, source_name)?;
        full_source = fs;
        stdlib_form_count = count;
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
        &mut compile,
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
    //
    // A stubbed closure is served by `call_precached_closure`, which runs it in
    // a FRESH `Store`. A fresh store cannot participate in a suspend/resume
    // chain — the `WasmSuspensionFrame` deque and env-stack snapshots live on
    // the full-module store, and `call_precached_closure` neither saves a frame
    // on suspend nor drives the resume chain. So a closure that may suspend
    // (yield, I/O, the async scheduler) loses its fiber state when precached,
    // corrupting any live state it held across the suspend. Precaching is
    // all-or-nothing (partial stubbing breaks the funcref table indices a
    // precached closure would need to call a non-precached sibling), so precache
    // only when NO closure in the module may suspend. The full stdlib's `ev/run`
    // scheduler suspends, so full-module programs keep every closure inline.
    let engine = store::create_engine().map_err(|e| e.to_string())?;
    let mut precached: Vec<Option<host::PrecachedClosure>> = vec![None; lir_module.closures.len()];
    let mut stubbed = std::collections::HashSet::new();

    let any_may_suspend = lir_module.closures.iter().any(|c| c.signal.may_suspend());
    if crate::config::get().cache.is_some() && !any_may_suspend {
        let mut all_ok = true;
        for (i, closure_func) in lir_module.closures.iter().enumerate() {
            if let Some(standalone) =
                emit::emit_single_closure(closure_func, Some(&lir_module), vm.heap_ptr, sym_ptr)
            {
                if let Ok(module) = compile_or_cache_module(&engine, &standalone.wasm_bytes) {
                    precached[i] = Some(host::PrecachedClosure {
                        module,
                        const_pool: standalone.const_pool,
                        env_stack_base: standalone.env_stack_base,
                    });
                    stubbed.insert(crate::lir::ClosureId(i as u32));
                } else {
                    all_ok = false;
                    break;
                }
            } else {
                all_ok = false;
                break;
            }
        }
        // If any closure failed to precache, fall back to full-module
        // dispatch for all closures. Partial precaching causes table
        // index mismatches when a precached closure calls a non-precached one.
        if !all_ok {
            precached.iter_mut().for_each(|p| *p = None);
            stubbed.clear();
        }
    }

    // LIR → WASM bytes + constant pool. Stubbed closures get minimal
    // bodies (unreachable) since they're served by pre-compiled Modules.
    let result = emit::emit_module(&lir_module, stubbed, vm.heap_ptr, sym_ptr);
    let t2 = std::time::Instant::now();

    // Dump WASM for analysis. /dev/shm (a tmpfs) rather than /tmp: the latter
    // is a shared, size-limited filesystem this process must not scribble into.
    if crate::config::get().wasm_dump {
        std::fs::write(WASM_DUMP_PATH, &result.wasm_bytes).ok();
    }

    let mut wasm_store = store::create_store(
        &engine,
        result.const_pool,
        result.closure_bytecodes,
        result.env_stack_base,
    );
    // This standalone eval owns its driving VM (built above); thread it to the
    // host so primitive calls build a VM-bearing `NativeCtx`.
    wasm_store.data_mut().vm = &mut vm as *mut crate::vm::VM;
    wasm_store.data_mut().precached_closures = precached;
    let linker = linker::create_linker(&engine).map_err(|e| e.to_string())?;
    let t3 = std::time::Instant::now();

    let cache_path = store::cache_path_for("module", &result.wasm_bytes);
    let module = store::cached_or_compile(&engine, &result.wasm_bytes, cache_path.as_deref())
        .map_err(|e| e.to_string())?;
    let t4 = std::time::Instant::now();
    let ret = store::run_module(&linker, &mut wasm_store, &module).map_err(|e| e.to_string());
    let t5 = std::time::Instant::now();

    // Materialize the result's display form NOW, while the heap is still alive.
    // A heap-valued result (an array, a list, a string) is a pointer into this
    // function's heap, which drops on return — so returning the `Value` would hand
    // the caller a dangling pointer it derefs when it formats the value. The CLI
    // discards the result (program output comes from `println` side effects during
    // execution), but test harnesses format it; returning the owned string keeps
    // them sound for heap results, not only immediates. Pinned by the `wasm_smoke`
    // / `wasm_stdlib` heap-result tests (e.g. `[1 2 3]`, `(map … (list 1 2 3))`).
    // Render through this instance's table: a bare `Display` prints a symbol or
    // keyword as `#<symbol:hash>` (docs/impl/symbol.md § "Reading a name, and
    // not reading one").
    let ret = ret.map(|v| format!("{}", v.display_with(Some(&symbols))));

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
