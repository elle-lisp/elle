use super::higher_order_def::define_higher_order_functions;
use super::time_def::define_time_functions;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Initialize the standard library
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    define_higher_order_functions(vm, symbols);
    define_time_functions(vm, symbols);
    define_vm_query_wrappers(vm, symbols);
    // Graph functions temporarily disabled while sorting out compilation caching.
    // define_graph_functions(vm, symbols);
}

/// Define Elle wrappers around vm/query operations
fn define_vm_query_wrappers(vm: &mut VM, symbols: &mut SymbolTable) {
    use crate::pipeline::eval;
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
