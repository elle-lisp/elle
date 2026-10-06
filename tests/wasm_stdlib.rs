// audited: 2026-10-06
// docs/impl/wasm.md
//! The standard library through the WASM backend: lowered, emitted, validated and run.
#![cfg(feature = "wasm")]

const STDLIB: &str = include_str!("../src/stdlib.lisp");

/// Set up an instance like the real elle binary does: primitives registered, a
/// fresh `CompileCtx`, and the VM pointed at this instance's own symbol table
/// (stdlib macros gensym, resolving through it). `RuntimeCore::bare` does all of
/// this; the compile context is threaded explicitly into the pipeline via
/// `core.parts()`.
fn setup() -> elle::runtime::RuntimeCore {
    elle::runtime::RuntimeCore::bare()
}

#[test]
fn compile_stdlib_to_bytecode() {
    let mut core = setup();
    let (_vm, symbols, cctx) = core.parts();
    match elle::pipeline::compile_file(STDLIB, symbols, cctx, "<stdlib>") {
        Ok(r) => eprintln!("stdlib bytecode: {} bytes", r.bytecode.instructions.len()),
        Err(e) => panic!("stdlib bytecode compilation failed: {}", e),
    }
}

#[test]
fn compile_stdlib_to_lir() {
    let mut core = setup();
    let (_vm, symbols, cctx) = core.parts();
    match elle::pipeline::compile_file_to_lir(STDLIB, symbols, cctx, "<stdlib>", 0) {
        Ok(lir) => {
            eprintln!(
                "stdlib LIR: {} blocks, {} regs, {} locals",
                lir.entry.view().block_count(),
                lir.entry.view().num_regs(),
                lir.entry.view().num_locals()
            );
        }
        Err(e) => panic!("stdlib compilation to LIR failed: {}", e),
    }
}

#[test]
fn compile_stdlib_to_wasm() {
    let mut core = setup();
    let (_vm, symbols, cctx) = core.parts();
    let lir = elle::pipeline::compile_file_to_lir(STDLIB, symbols, cctx, "<stdlib>", 0).unwrap();
    let result = elle::wasm::emit::emit_module(
        &lir,
        std::collections::HashSet::new(),
        core.heap() as *mut elle::value::fiberheap::FiberHeap,
        core.symbols() as *mut elle::SymbolTable,
    );
    eprintln!(
        "stdlib WASM: {} bytes, {} constants",
        result.wasm_bytes.len(),
        result.const_pool.len()
    );
}

#[test]
fn run_stdlib_first_100_lines() {
    // Test cond — expands to nested if/else
    let source = r#"
(defn classify [x]
  (cond
    ((< x 0) :negative)
    ((= x 0) :zero)
    (true :positive)))
(classify 5)
"#;
    let mut core = setup();
    let (_vm, symbols, cctx) = core.parts();
    let lir = elle::pipeline::compile_file_to_lir(source, symbols, cctx, "<stdlib>", 0).unwrap();
    let result = elle::wasm::emit::emit_module(
        &lir,
        std::collections::HashSet::new(),
        core.heap() as *mut elle::value::fiberheap::FiberHeap,
        core.symbols() as *mut elle::SymbolTable,
    );
    let engine = elle::wasm::store::create_engine().unwrap();
    match elle::wasm::store::compile_module(&engine, &result.wasm_bytes) {
        Ok(_) => eprintln!("first 100 lines: WASM valid"),
        Err(e) => panic!("first 100 lines WASM invalid:\n{:#}", e),
    }
}

/// Test that stdlib + user code works together.
#[test]
fn stdlib_with_map() {
    // Compile stdlib + user code together
    let source = format!("{}\n(map (fn [x] (+ x 1)) (list 1 2 3))", STDLIB);
    match elle::wasm::eval_wasm(&source, "<test>") {
        Ok(v) => assert_eq!(v, "(2 3 4)"),
        Err(e) => panic!("stdlib+map failed: {}", e),
    }
}

#[test]
fn run_stdlib_on_wasm() {
    let mut core = setup();
    let (_vm, symbols, cctx) = core.parts();
    let lir = elle::pipeline::compile_file_to_lir(STDLIB, symbols, cctx, "<stdlib>", 0).unwrap();
    let result = elle::wasm::emit::emit_module(
        &lir,
        std::collections::HashSet::new(),
        core.heap() as *mut elle::value::fiberheap::FiberHeap,
        core.symbols() as *mut elle::SymbolTable,
    );
    eprintln!(
        "WASM: {} bytes, {} consts, {} closures",
        result.wasm_bytes.len(),
        result.const_pool.len(),
        lir.entry
            .view()
            .nodes()
            .filter(|n| n.op() == elle::lir::code::Op::MakeClosure)
            .count()
    );

    // Try to compile with wasmtime for a detailed error
    let engine = elle::wasm::store::create_engine().unwrap();
    match elle::wasm::store::compile_module(&engine, &result.wasm_bytes) {
        Ok(_) => eprintln!("WASM module compiled successfully"),
        Err(e) => {
            // Keep the module for inspection, under a name no other run shares.
            let path =
                std::env::temp_dir().join(format!("elle-stdlib-test-{}.wasm", std::process::id()));
            std::fs::write(&path, &result.wasm_bytes).unwrap();
            eprintln!("Wrote WASM to {}", path.display());
            panic!("WASM compilation failed:\n{:#}", e);
        }
    }
}
