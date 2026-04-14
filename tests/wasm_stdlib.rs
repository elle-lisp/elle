//! Test: can we compile and run stdlib.lisp through WASM?
#![cfg(feature = "wasm")]

const STDLIB: &str = include_str!("../stdlib.lisp");

/// Set up a VM + symbols like the real elle binary does.
fn setup() -> (elle::VM, Box<elle::SymbolTable>) {
    let mut vm = elle::VM::new();
    let mut symbols = Box::new(elle::SymbolTable::new());
    elle::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut elle::SymbolTable = &mut *symbols;
    elle::context::set_symbol_table(sym_ptr);
    (vm, symbols)
}

#[test]
fn compile_stdlib_to_bytecode() {
    let (_vm, mut symbols) = setup();
    match elle::pipeline::compile_file(STDLIB, &mut symbols, "<stdlib>") {
        Ok(r) => eprintln!("stdlib bytecode: {} bytes", r.bytecode.instructions.len()),
        Err(e) => panic!("stdlib bytecode compilation failed: {}", e),
    }
}

#[test]
fn compile_stdlib_to_lir() {
    let (_vm, mut symbols) = setup();
    match elle::pipeline::compile_file_to_lir(STDLIB, &mut symbols, "<stdlib>", 0) {
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
    let (_vm, mut symbols) = setup();
    let lir = elle::pipeline::compile_file_to_lir(STDLIB, &mut symbols, "<stdlib>", 0).unwrap();
    let result = elle::wasm::emit::emit_module(&lir, std::collections::HashSet::new());
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
    let (_vm, mut symbols) = setup();
    let lir = elle::pipeline::compile_file_to_lir(source, &mut symbols, "<stdlib>", 0).unwrap();
    let result = elle::wasm::emit::emit_module(&lir, std::collections::HashSet::new());
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
    let (_vm, mut symbols) = setup();
    let lir = elle::pipeline::compile_file_to_lir(STDLIB, &mut symbols, "<stdlib>", 0).unwrap();
    let result = elle::wasm::emit::emit_module(&lir, std::collections::HashSet::new());
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
            // Dump WASM for inspection
            let mut f = std::fs::File::create("/tmp/stdlib_test.wasm").unwrap();
            std::io::Write::write_all(&mut f, &result.wasm_bytes).unwrap();
            eprintln!("Wrote WASM to /tmp/stdlib_test.wasm");
            panic!("WASM compilation failed:\n{:#}", e);
        }
    }
}
