use super::higher_order_def::define_higher_order_functions;
use super::time_def::define_time_functions;
use crate::pipeline::eval;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Initialize the standard library
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    // Define Lisp implementations of higher-order functions that support closures
    define_higher_order_functions(vm, symbols);
    define_time_functions(vm, symbols);
    define_vm_query_wrappers(vm, symbols);
    define_file_functions(vm, symbols);
}

/// Define file-related functions implemented in Elle
fn define_file_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    let defs = [
        // import: read a file, parse all forms, wrap in begin, eval
        r#"(def import (fn (filename) (eval (cons 'begin (read-all (slurp filename))))))"#,
    ];
    for code in &defs {
        if let Err(e) = eval(code, symbols, vm) {
            eprintln!("Warning: Failed to define file function: {}", e);
        }
    }
}

/// Define Elle wrappers around vm/query operations
fn define_vm_query_wrappers(vm: &mut VM, symbols: &mut SymbolTable) {
    let defs = [
        r#"(def call-count (fn (f) (vm/query "call-count" f)))"#,
        r#"(def global? (fn (sym) (vm/query "global?" sym)))"#,
        r#"(def fiber/self (fn () (vm/query "fiber/self" nil)))"#,
    ];
    for code in &defs {
        if let Err(e) = eval(code, symbols, vm) {
            eprintln!("Warning: Failed to define vm/query wrapper: {}", e);
        }
    }
}
