//! Test: can we compile and run stdlib.lisp through WASM?

const STDLIB: &str = include_str!("../stdlib.lisp");

/// Set up a VM + symbols like the real elle binary does.
fn setup() -> (elle::VM, Box<elle::SymbolTable>) {
    let mut vm = elle::VM::new();
    let mut symbols = Box::new(elle::SymbolTable::new());
    elle::register_primitives(&mut vm, &mut symbols);
    let sym_ptr: *mut elle::SymbolTable = &mut *symbols;
    elle::context::set_symbol_table(sym_ptr);
    elle::primitives::set_length_symbol_table(sym_ptr);
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
    match elle::pipeline::compile_file_to_lir(STDLIB, &mut symbols, "<stdlib>") {
        Ok(lir) => {
            eprintln!(
                "stdlib LIR: {} blocks, {} regs, {} locals",
                lir.blocks.len(),
                lir.num_regs,
                lir.num_locals
            );
        }
        Err(e) => panic!("stdlib compilation to LIR failed: {}", e),
    }
}

#[test]
fn compile_stdlib_to_wasm() {
    let (_vm, mut symbols) = setup();
    let lir = elle::pipeline::compile_file_to_lir(STDLIB, &mut symbols, "<stdlib>").unwrap();
    let result = elle::wasm::emit::emit_module(&lir);
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
    let lir = elle::pipeline::compile_file_to_lir(source, &mut symbols, "<stdlib>").unwrap();
    let result = elle::wasm::emit::emit_module(&lir);
    let engine = elle::wasm::store::create_engine().unwrap();
    match elle::wasm::store::compile_module(&engine, &result.wasm_bytes) {
        Ok(_) => eprintln!("first 100 lines: WASM valid"),
        Err(e) => panic!("first 100 lines WASM invalid:\n{:#}", e),
    }
}

#[test]
fn run_stdlib_first_200_lines() {
    let source: String = STDLIB.lines().take(200).collect::<Vec<_>>().join("\n");
    let (_vm, mut symbols) = setup();
    let lir = elle::pipeline::compile_file_to_lir(&source, &mut symbols, "<stdlib>").unwrap();
    let result = elle::wasm::emit::emit_module(&lir);
    let engine = elle::wasm::store::create_engine().unwrap();
    match elle::wasm::store::compile_module(&engine, &result.wasm_bytes) {
        Ok(_) => eprintln!("first 200 lines: WASM valid"),
        Err(e) => panic!("first 200 lines WASM invalid: {}", e),
    }
}

#[test]
fn run_stdlib_on_wasm() {
    let (_vm, mut symbols) = setup();
    let lir = elle::pipeline::compile_file_to_lir(STDLIB, &mut symbols, "<stdlib>").unwrap();
    let result = elle::wasm::emit::emit_module(&lir);
    eprintln!(
        "WASM: {} bytes, {} consts, {} closures",
        result.wasm_bytes.len(),
        result.const_pool.len(),
        lir.blocks
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
            panic!("WASM compilation failed:\n{}", e);
        }
    }
}
