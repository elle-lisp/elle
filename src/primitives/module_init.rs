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
const STDLIB: &str = include_str!("../stdlib.lisp");
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
/// Layout: `[captures..., params..., locals...]` — matches `populate_env`
/// (`src/vm/env.rs`).  `LoadUpvalue` indexes the env from zero, so the
/// captures must come first; any local slots reserved by the closure
/// (including ANF-lifted temporaries) sit at the tail of the buffer and
/// are filled by the runtime as the body executes.
pub fn build_closure_call_env(closure: &crate::value::Closure, args: &[Value]) -> Vec<Value> {
    let template = &closure.template;
    let num_locally_defined = template.num_locals.saturating_sub(template.num_params);
    let total = closure.env.len() + template.num_params + num_locally_defined;
    let mut env = Vec::with_capacity(total);
    env.extend(closure.env.iter().copied());
    for i in 0..template.num_params {
        env.push(args.get(i).copied().unwrap_or(Value::NIL));
    }
    for _ in 0..num_locally_defined {
        env.push(Value::NIL);
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
                Signal::silent()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::compile_file;
    use crate::primitives::registration::register_primitives;

    #[test]
    fn build_closure_call_env_places_captures_before_locals() {
        // Regression test for Finding 4.
        //
        // `build_closure_call_env` constructs the env that the stdlib's
        // tail export closure receives at call time. The VM's
        // `LoadUpvalue` instruction indexes the env from zero, so the
        // captures must sit at the front. The old layout reserved
        // `num_locals` nil slots in front of the captures — invisible
        // while `num_locals == 0` (its assumed state for a trivial
        // `(fn [] {...})`), but the ANF lift introduces one local for
        // every allocating subexpression in the closure body. The stdlib
        // export closure contains an inline `(fn [port] ...)`, which the
        // lift names into a local. That local then occupied env[0] and
        // shifted every capture by one slot, so every capture read as
        // nil — which is why `(+ 1 2)` came back nil during
        // `init_stdlib`.
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        let _ = register_primitives(&mut vm, &mut symbols);

        // A captured outer binding (`outer`) and an allocating let
        // inside the returned closure (`(fn [x] x)` allocates a
        // closure → ANF lifts it into a local).
        let source = "(letrec [outer (fn [n] n)] \
                      (fn [] (let [inner (fn [x] x)] outer)))";
        let compiled = compile_file(source, &mut symbols, "<test>").expect("source must compile");
        let closure_val = vm
            .execute(&compiled.bytecode)
            .expect("top-level execution must succeed");
        let closure = closure_val
            .as_closure()
            .expect("top-level must evaluate to a closure");

        assert!(
            closure.template.num_captures >= 1,
            "the test source must produce a closure with captures; got num_captures={}",
            closure.template.num_captures
        );
        assert!(
            closure.template.num_locals >= 1,
            "the test source must produce a closure with at least one local \
             (otherwise the bug condition isn't exercised); got num_locals={}",
            closure.template.num_locals
        );

        let env = build_closure_call_env(closure, &[]);
        assert!(
            !env[0].is_nil(),
            "env[0] must be the first capture — a nil here means locals \
             were placed before captures and `LoadUpvalue(0)` reads nil. \
             num_captures={}, num_locals={}, env[0]={}",
            closure.template.num_captures,
            closure.template.num_locals,
            env[0],
        );
    }
}
