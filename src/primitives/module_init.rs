use crate::pipeline::eval_all;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../../stdlib.lisp");

/// Initialize the standard library by evaluating stdlib.lisp.
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    if let Err(e) = eval_all(STDLIB, symbols, vm) {
        panic!("stdlib loading failed: {}", e);
    }
}
