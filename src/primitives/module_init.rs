use super::higher_order_def::define_higher_order_functions;
use super::time_def::define_time_functions;
use crate::pipeline::eval;
use crate::symbol::{ModuleDef, SymbolTable};
use crate::vm::VM;
use std::collections::HashMap;

/// Initialize the standard library
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    // Define Lisp implementations of higher-order functions that support closures
    define_higher_order_functions(vm, symbols);
    define_time_functions(vm, symbols);
    define_vm_query_wrappers(vm, symbols);

    init_list_module(vm, symbols);
    init_string_module(vm, symbols);
    init_math_module(vm, symbols);
    init_json_module(vm, symbols);
    init_clock_module(vm, symbols);
    init_time_module(vm, symbols);
}

/// Initialize the list module
fn init_list_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut list_exports = HashMap::new();

    let functions = vec![
        "length", "empty?", "append", "reverse", "map", "filter", "fold", "nth", "last", "take",
        "drop", "list", "cons", "first", "rest",
    ];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            list_exports.insert(symbols.intern(func_name).0, *func);
        }
        exports.push(symbols.intern(func_name));
    }

    let list_module = ModuleDef {
        name: symbols.intern("list"),
        exports,
    };
    symbols.define_module(list_module);
    vm.define_module("list".to_string(), list_exports);
}

/// Initialize the string module
fn init_string_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut string_exports = HashMap::new();

    let functions = vec![
        "string-append",
        "string-upcase",
        "string-downcase",
        "substring",
        "string-index",
        "char-at",
        "string",
        "string-split",
        "string-replace",
        "string-trim",
        "string-contains?",
        "string-starts-with?",
        "string-ends-with?",
        "string-join",
        "number->string",
    ];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            string_exports.insert(symbols.intern(func_name).0, *func);
        }
        exports.push(symbols.intern(func_name));
    }

    let string_module = ModuleDef {
        name: symbols.intern("string"),
        exports,
    };
    symbols.define_module(string_module);
    vm.define_module("string".to_string(), string_exports);
}

/// Initialize the math module
fn init_math_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut math_exports = HashMap::new();

    let functions = vec![
        "+", "-", "*", "/", "mod", "rem", "abs", "min", "max", "sqrt", "sin", "cos", "tan", "log",
        "exp", "pow", "floor", "ceil", "round", "even?", "odd?", "pi", "e",
    ];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            math_exports.insert(symbols.intern(func_name).0, *func);
        }
        exports.push(symbols.intern(func_name));
    }

    let math_module = ModuleDef {
        name: symbols.intern("math"),
        exports,
    };
    symbols.define_module(math_module);
    vm.define_module("math".to_string(), math_exports);
}

/// Initialize the JSON module
fn init_json_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut json_exports = HashMap::new();

    let functions = vec!["json-parse", "json-serialize", "json-serialize-pretty"];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            json_exports.insert(symbols.intern(func_name).0, *func);
        }
        exports.push(symbols.intern(func_name));
    }

    let json_module = ModuleDef {
        name: symbols.intern("json"),
        exports,
    };
    symbols.define_module(json_module);
    vm.define_module("json".to_string(), json_exports);
}

/// Initialize the clock module
fn init_clock_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut clock_exports = HashMap::new();

    let functions = vec!["clock/monotonic", "clock/realtime", "clock/cpu"];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            clock_exports.insert(symbols.intern(func_name).0, *func);
        }
        exports.push(symbols.intern(func_name));
    }

    let clock_module = ModuleDef {
        name: symbols.intern("clock"),
        exports,
    };
    symbols.define_module(clock_module);
    vm.define_module("clock".to_string(), clock_exports);
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

/// Initialize the time module
fn init_time_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut time_exports = HashMap::new();

    let functions = vec!["time/sleep", "time/stopwatch", "time/elapsed"];

    let mut exports = Vec::new();
    for func_name in &functions {
        if let Some(func) = vm.get_global(symbols.intern(func_name).0) {
            time_exports.insert(symbols.intern(func_name).0, *func);
        }
        exports.push(symbols.intern(func_name));
    }

    let time_module = ModuleDef {
        name: symbols.intern("time"),
        exports,
    };
    symbols.define_module(time_module);
    vm.define_module("time".to_string(), time_exports);
}
