//! Test: can we compile and run stdlib.lisp through WASM?
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
                lir.entry.blocks.len(),
                lir.entry.num_regs,
                lir.entry.num_locals
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
        Ok(v) => assert_eq!(format!("{}", v), "(2 3 4)"),
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
    );
    eprintln!(
        "WASM: {} bytes, {} consts, {} closures",
        result.wasm_bytes.len(),
        result.const_pool.len(),
        lir.entry
            .blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|i| matches!(i.instr, elle::lir::LirInstr::MakeClosure { .. }))
            .count()
    );

    // Try to compile with wasmtime for a detailed error
    let engine = elle::wasm::store::create_engine().unwrap();
    match elle::wasm::store::compile_module(&engine, &result.wasm_bytes) {
        Ok(_) => eprintln!("WASM module compiled successfully"),
        Err(e) => {
            // Dump WASM for inspection (/dev/shm — /tmp is off-limits here)
            let mut f = std::fs::File::create("/dev/shm/stdlib_test.wasm").unwrap();
            std::io::Write::write_all(&mut f, &result.wasm_bytes).unwrap();
            eprintln!("Wrote WASM to /dev/shm/stdlib_test.wasm");
            panic!("WASM compilation failed:\n{:#}", e);
        }
    }
}
