use crate::pipeline::compile_all;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../../stdlib.lisp");

/// Initialize the standard library by evaluating stdlib.lisp.
///
/// Uses `compile_all` (the old multi-form pipeline) because stdlib
/// definitions must persist as globals in `vm.globals` — they need
/// to be visible to all subsequent compilations. The file-as-letrec
/// model (`compile_file`) creates local bindings that don't survive
/// past execution.
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    let results = match compile_all(STDLIB, symbols) {
        Ok(r) => r,
        Err(e) => panic!("stdlib compilation failed: {}", e),
    };
    for result in results {
        if let Err(e) = vm.execute(&result.bytecode) {
            panic!("stdlib execution failed: {}", e);
        }
    }
}
