use crate::pipeline::compile_file;
use crate::pipeline::update_cache_with_stdlib;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use crate::value::Value;
use crate::vm::VM;
use std::collections::HashMap;
use std::rc::Rc;

/// Standard library source, embedded at compile time.
const STDLIB: &str = include_str!("../../stdlib.lisp");

/// Initialize the standard library by evaluating stdlib.lisp.
///
/// The stdlib is compiled as a single synthetic letrec so that
/// definitions are visible to subsequent forms (mutual recursion).
/// The last expression is a closure returning a struct of all exports.
/// We call that closure, iterate the exports struct, and register each
/// export into the compilation cache's PrimitiveMeta so that
/// `bind_primitives` pre-binds them for all subsequent compilations.
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    let result = match compile_file(STDLIB, symbols, "<stdlib>") {
        Ok(r) => r,
        Err(e) => panic!("stdlib compilation failed: {}", e),
    };

    // Execute stdlib — returns the last expression (a closure).
    let closure_val = match vm.execute(&result.bytecode) {
        Ok(v) => v,
        Err(e) => panic!("stdlib execution failed: {}", e),
    };

    // Call the returned closure to get the exports struct.
    let exports_val = call_closure(vm, closure_val);

    // Extract exports from the struct and register them.
    let exports = extract_exports(exports_val, symbols);
    register_stdlib_exports(vm, symbols, &exports);
}

/// Call a zero-argument closure and return its result.
fn call_closure(vm: &mut VM, closure_val: Value) -> Value {
    let closure = closure_val
        .as_closure()
        .unwrap_or_else(|| panic!("stdlib last expression is not a closure: {}", closure_val));

    let env = Rc::new(build_closure_call_env(closure, &[]));

    match vm.execute_bytecode(
        &closure.template.bytecode,
        &closure.template.constants,
        Some(&env),
    ) {
        Ok(v) => v,
        Err(e) => panic!("stdlib export closure call failed: {}", e),
    }
}

/// Build the local environment for calling a closure with the given args.
///
/// Layout: [params..., locals..., captures...]
/// For a zero-arg closure: [locals..., captures...]
fn build_closure_call_env(closure: &crate::value::Closure, args: &[Value]) -> Vec<Value> {
    let template = &closure.template;
    let total = template.num_locals + template.num_captures;
    let mut env = vec![Value::NIL; total];

    // Copy args into param slots
    for (i, arg) in args.iter().enumerate() {
        if i < total {
            env[i] = *arg;
        }
    }

    // Copy captures into capture slots (after locals)
    let capture_start = template.num_locals;
    for (i, cap) in closure.env.iter().enumerate() {
        if capture_start + i < total {
            env[capture_start + i] = *cap;
        }
    }

    env
}

/// Extract keyword→value pairs from an exports struct.
///
/// Reads the signal directly from each exported value's compiled representation.
fn extract_exports(
    exports_val: Value,
    symbols: &mut SymbolTable,
) -> HashMap<SymbolId, (Value, Signal)> {
    let exports_struct = exports_val.as_struct().unwrap_or_else(|| {
        panic!(
            "stdlib export closure did not return a struct: {}",
            exports_val
        )
    });

    let mut result = HashMap::new();
    for (key, value) in exports_struct.iter() {
        if let crate::value::types::TableKey::Keyword(name) = key {
            let sym_id = symbols.intern(name);
            let signal = if let Some(closure) = value.as_closure() {
                closure.template.signal
            } else if value.is_parameter() {
                Signal::inert()
            } else {
                panic!(
                    "stdlib export '{}' is neither closure nor parameter: {}",
                    name, value
                )
            };
            result.insert(sym_id, (*value, signal));
        }
    }
    result
}

/// Register stdlib exports into the compilation caches.
///
/// In the letrec model there are no VM globals. Stdlib exports are
/// made available to user code via `bind_primitives`, which reads
/// from `PrimitiveMeta.functions` and `PrimitiveMeta.signals`.
fn register_stdlib_exports(
    _vm: &mut VM,
    symbols: &mut SymbolTable,
    exports: &HashMap<SymbolId, (Value, Signal)>,
) {
    // Update the compilation cache so subsequent compile_file calls
    // see stdlib exports as primitives.
    update_cache_with_stdlib(exports.clone());

    // Update the standalone primitive meta cache too (used by eval, eval_syntax).
    crate::primitives::registration::update_primitive_meta_cache(exports);

    // Intern all stdlib export names in the symbol table.
    for sym_id in exports.keys() {
        // Already interned by extract_exports, but ensure the caller's
        // symbol table has them too.
        let _ = symbols.name(*sym_id);
    }
}
