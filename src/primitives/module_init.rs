use crate::pipeline::compile_file;
use crate::pipeline::CompileCtx;
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
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) {
    let profile = std::env::var("ELLE_PROFILE").is_ok();
    let t0 = std::time::Instant::now();
    let mark = |label: &str| {
        if profile {
            eprintln!("[elle-profile] stdlib {label}: {:?}", t0.elapsed());
        }
    };
    // Try the disk cache first: same stdlib source + same elle binary → same
    // compiled bytecode. On hit we skip the entire ~2.4s front end.
    if let Some(cached) = crate::compiler::stdlib_cache::try_load(STDLIB, vm, symbols, cctx) {
        let bytecode = match cached {
            Ok(bc) => {
                mark("cache-load");
                if profile {
                    eprintln!("[elle-profile] stdlib cache HIT");
                }
                bc
            }
            Err(e) => {
                if profile {
                    eprintln!("[elle-profile] stdlib cache miss ({}); recompiling", e);
                }
                match compile_file(STDLIB, symbols, cctx, "<stdlib>") {
                    Ok(r) => r.bytecode,
                    Err(e) => panic!("stdlib compilation failed: {}", e),
                }
            }
        };
        // Execute stdlib — returns the last expression (a closure).
        let closure_val = match vm.execute(&bytecode) {
            Ok(v) => v,
            Err(e) => panic!("stdlib execution failed: {}", e),
        };
        mark("executed");
        // Call the returned closure to get the exports struct.
        let exports_val = call_closure(vm, closure_val);
        mark("exports");
        register_exports(vm, symbols, cctx, closure_val, exports_val);
        return;
    }
    let result = match compile_file(STDLIB, symbols, cctx, "<stdlib>") {
        Ok(r) => r,
        Err(e) => panic!("stdlib compilation failed: {}", e),
    };
    mark("compiled");
    // Persist the compiled bytecode so the next process start skips the front
    // end entirely. A write failure is not fatal — the cache is a speedup, and
    // a fresh compile is always the fallback.
    crate::compiler::stdlib_cache::try_store(STDLIB, &result.bytecode, vm, symbols, cctx);
    mark("cache-store");
    // Execute stdlib — returns the last expression (a closure).
    let closure_val = match vm.execute(&result.bytecode) {
        Ok(v) => v,
        Err(e) => panic!("stdlib execution failed: {}", e),
    };
    mark("executed");
    // Call the returned closure to get the exports struct.
    let exports_val = call_closure(vm, closure_val);
    mark("exports");
    register_exports(vm, symbols, cctx, closure_val, exports_val);
}

/// Register each stdlib export into the compilation cache (the tail of the
/// original `init_stdlib`, split out so both the cached and compiled paths
/// share it).
fn register_exports(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    closure_val: Value,
    exports_val: Value,
) {
    // Root the stdlib export aggregate (the struct + its module closure), not
    // each export. `exports_val` references every stdlib export, and the `Value`s
    // registered into the compilation caches below are aliases into those
    // regions. Under the mint-at-return convention the struct lives on the
    // top-level return mint's +1, which the caller (here, init_stdlib) is
    // responsible for balancing at the result's decref_point — so without a root
    // the struct would be freed there and cascade-free the exports while the
    // caches still alias them. Rooting the aggregate keeps the exports live for
    // the process and lets teardown reclaim them by RC cascade. Distinct
    // regions, one registration each (R9).
    crate::value::arena::register_process_root(unsafe { &mut *vm.heap_ptr }, closure_val);
    crate::value::arena::register_process_root(unsafe { &mut *vm.heap_ptr }, exports_val);
    // Extract exports from the struct and register them.
    let exports = extract_exports(exports_val, symbols);
    register_stdlib_exports(cctx, symbols, &exports);
    // Arm guardfree page-protection (no-op unless --trace=guardfree): from
    // here on, freed pages are mprotected so the first user-program
    // use-after-free faults at the exact dereference. Stdlib init has its
    // own benign init-time frees, excluded by arming only now.
    crate::value::fiberheap::freelog::arm_guard();
}
/// Call a zero-argument closure and return its result.
fn call_closure(vm: &mut VM, closure_val: Value) -> Value {
    let closure = closure_val
        .as_closure()
        .unwrap_or_else(|| panic!("stdlib last expression is not a closure: {}", closure_val));
    let env = Rc::new(build_closure_call_env(closure, &[]));
    // The body being run is a closure's — hand it its executing-closure
    // register via the one-shot, and enter through `execute_code` with the
    // template's own `Code` (sharing its bytecode `Rc`, which the dispatch-entry
    // invariant compares by identity).
    vm.pending_entry_closure = closure_val;
    match vm.execute_code(closure.template.code(), Some(&env)) {
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
    cctx: &mut CompileCtx,
    symbols: &mut SymbolTable,
    exports: &HashMap<SymbolId, (Value, Signal)>,
) {
    // Add the exports to this instance's compile context: user code sees them as
    // globals (`meta`), and macro-transformer bodies can call them (`eval_meta`).
    cctx.register_stdlib_exports(exports);
    // Intern all stdlib export names in the symbol table.
    for sym_id in exports.keys() {
        // Already interned by extract_exports, but ensure the caller's
        // symbol table has them too.
        let _ = symbols.name(*sym_id);
    }
}

#[cfg(test)]
mod tests;
