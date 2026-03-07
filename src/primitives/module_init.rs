use crate::pipeline::compile_all;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../../stdlib.lisp");

/// Initialize the standard library by evaluating stdlib.lisp.
///
/// Each top-level form is compiled and executed independently so that
/// `def` bindings persist as globals in `vm.globals`.
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
