//! Regression guard: the LSP's resident-VM analysis path is region-clean.
//!
//! The use-after-free "demonstrated by lsp" is NOT in the LSP's own usage
//! pattern. The LSP holds
//! ONE resident VM (`CompilerState`, src/lsp/state.rs) that runs `init_stdlib`
//! exactly once, then loops `analyze_file` per document change. That pattern is
//! region-safe. This test reproduces it directly with `--trace=guardfree`
//! armed (the reliable UAF oracle: freed pages are mprotect(PROT_NONE)'d and
//! never reused, so the FIRST real use-after-free SIGSEGVs at the exact deref).
//!
//! Macro-heavy inputs are used on purpose: nested macro expansion (`forever`,
//! `try`, `defer`, `with`, user `defmacro`) is the documented trigger for the
//! lazily-compiled-transformer transient-region free. If a future change frees
//! a live region on the resident analyze path, guardfree turns this clean run
//! into a SIGSEGV and this test fails.
//!
//! This file is its OWN test binary so the process-global `config::init`
//! (which arms guardfree) cannot leak into the shared integration-test binary.

use elle::{analyze_file, init_stdlib, register_primitives, SymbolTable, VM};

#[test]
fn lsp_resident_analyze_is_region_clean_under_guardfree() {
    // Arm guardfree process-wide BEFORE building the VM. `init_stdlib` reads
    // this and calls `arm_guard()` after stdlib load (module_init.rs), so the
    // benign init-time frees are skipped and only post-init UAFs fault.
    let mut cfg = elle::config::Config::default();
    cfg.trace_keywords.push("guardfree".to_string());
    elle::config::init(cfg);

    // Mirror CompilerState::new(): one resident VM, init_stdlib ONCE.
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _ = register_primitives(&mut vm, &mut symbols);
    // One resident per-instance compile context, threaded through stdlib load
    // and every `analyze_file` below — the resident-VM analogue of
    // `CompilerState`'s own `CompileCtx`, threaded as a parameter. Wire the VM
    // to it so stdlib-load-time `eval`/`import` resolve against this instance.
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_compile_ctx(&mut cctx as *mut elle::pipeline::CompileCtx);
    vm.set_symbols(&mut symbols as *mut SymbolTable);
    init_stdlib(
        &mut vm,
        &mut symbols,
        &mut cctx,
        &elle::compiler::stdlib_cache::StdlibCache::Off,
    );

    // Mirror compile_document(): re-analyze on every "change", reusing the
    // resident VM and its per-instance compile context across iterations.
    let texts = [
        "(forever (println 1))",
        "(try (+ 1 2) (catch e e))",
        "(defer (close x) (read x))",
        "(each x [1 2 3] (println x))",
        "(with p (open) (close p) (read p))",
        "(defmacro m [x] `(forever ,x)) (m (yield 1))",
        "(let [a 1 b 2] (+ a b))",
        "(map (fn [x] (* x x)) [1 2 3 4 5])",
    ];
    for i in 0..240 {
        let t = texts[i % texts.len()];
        // Errors (e.g. unbound `x`/`open`) are fine — we only care that
        // analysis never dereferences freed region memory.
        let _ = analyze_file(t, &mut symbols, &mut vm, &mut cctx, "<lsp-resident>");
    }
}
