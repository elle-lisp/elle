//! Runtime eval instruction handler.
//!
//! Compiles and executes a datum (quoted value) at runtime.
//! The expression is compiled in an environment seeded from primitives
//! and prelude. When the optional env argument is a non-nil struct,
//! its symbol-keyed entries become additional immutable bindings.

use crate::error::{LError, LResult};
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::lir::{Emitter, Lowerer};
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax};
use crate::value::heap::TableKey;
use crate::value::{Value, SIG_ERROR, SIG_OK};
use std::collections::HashMap;
use std::rc::Rc;

use super::core::VM;

/// Handle the Eval instruction from the dispatch loop.
///
/// Reaches this instance's symbol table through the driving VM (`vm.symbols_ptr`,
/// set by `RuntimeCore`). A bare VM with no `RuntimeCore` has none, and
/// `(eval …)` then errors (it needs a `Runtime`).
pub(crate) fn handle_eval_instruction(vm: &mut VM) {
    let expr_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on eval (expr)");
    // Pop the env argument from the stack (bytecode always pushes two
    // operands for Eval).
    let env_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on eval (env)");

    // This instance's symbol table, reached through the driving VM. Raw deref so
    // it sits beside the `&mut vm` passed to `eval_inner`.
    let symbols_ptr = vm.symbols_ptr;
    if symbols_ptr.is_null() {
        let err = vm.escaping_error(
            "eval-error",
            "eval: symbol table not available (eval requires a Runtime instance)",
        );
        vm.fiber.signal = Some((SIG_ERROR, err));
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    let symbols = unsafe { &mut *symbols_ptr };

    match eval_inner(vm, expr_value, env_value, symbols) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            let err = vm.escaping_error("eval-error", msg);
            vm.fiber.signal = Some((SIG_ERROR, err));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

fn eval_inner(
    vm: &mut VM,
    expr_value: Value,
    env_value: Value,
    symbols: &mut SymbolTable,
) -> LResult<Value> {
    // Convert value to Syntax
    let span = Span::synthetic();
    let syntax = Syntax::from_value(&expr_value, symbols, span)?;

    // The runtime `eval` instruction compiles against the owning instance's
    // compile context (core.lisp env + macro-body metadata). Cloned upfront so
    // the rest of this function can use `vm` freely.
    let Some((core_env, eval_meta)) = vm
        .compile_ctx()
        .map(|c| (c.core_env(), c.primitive_meta().clone()))
    else {
        return Err(LError::generic(
            "eval: no compile context (eval requires a Runtime instance)".to_string(),
        ));
    };

    // Get-or-create Expander (cached on VM). A fresh one inherits the
    // instance's core env and macro-body `eval_meta` so transformer bodies in
    // the evaluated code compile.
    let mut expander = match vm.eval_expander.take() {
        Some(e) => e,
        None => {
            let mut e = crate::syntax::Expander::new();
            e.core_env = core_env;
            e.set_eval_meta(eval_meta.clone());
            e
        }
    };

    // Save the caller's stack before macro expansion. load_prelude and
    // expand both execute VM bytecode (via eval_syntax → vm.execute)
    // which shares the same fiber stack. Without saving, macro expansion
    // overwrites the caller's local variable slots — corrupting cells
    // that hold destructured bindings.
    let saved_stack = std::mem::take(&mut vm.fiber.stack);

    // Load prelude if this is a fresh expander
    if !expander.has_macros() {
        match expander.load_prelude(symbols, vm) {
            Ok(_) => {}
            Err(e) => {
                vm.fiber.stack = saved_stack;
                vm.eval_expander = Some(expander);
                return Err(LError::generic(format!("eval: prelude load failed: {}", e)));
            }
        }
    }

    // Expand
    let expanded = match expander.expand(syntax, symbols, vm) {
        Ok(e) => e,
        Err(e) => {
            vm.fiber.stack = saved_stack;
            vm.eval_expander = Some(expander);
            return Err(LError::generic(format!("eval: expansion failed: {}", e)));
        }
    };

    // Restore the caller's stack after macro expansion
    vm.fiber.stack = saved_stack;

    // Put Expander back
    vm.eval_expander = Some(expander);

    // Extract env bindings before creating the analyzer (avoids borrow conflict)
    let env_map = if !env_value.is_nil() {
        Some(extract_env_bindings(env_value, symbols)?)
    } else {
        None
    };

    // Analyze
    let meta = eval_meta;
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    if let Some(ref exp) = vm.eval_expander {
        if !exp.core_env.is_empty() {
            analyzer.bind_compile_time_env(&exp.core_env, true);
        }
    }
    if let Some(ref env_map) = env_map {
        analyzer.bind_compile_time_env(env_map, false);
    }

    let mut analysis = analyzer
        .analyze(&expanded)
        .map_err(|e| LError::generic(format!("eval: analysis failed: {}", e)))?;
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);

    // Mark tail calls
    mark_tail_calls(&mut analysis.hir);
    crate::hir::functionalize::functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);

    // Lower
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let region_info =
        crate::hir::analyze_regions_with(&analysis.hir, &arena, pc.call_classification.clone());
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info);
    let lir_module = lowerer
        .lower(&analysis.hir)
        .map_err(|e| LError::generic(format!("eval: lowering failed: {}", e)))?;

    // Emit
    let mut emitter = Emitter::new();
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    // Execute
    let mut code = crate::value::Code::new(
        Rc::new(bytecode.instructions),
        Rc::new(bytecode.constants),
        Rc::new(bytecode.location_map),
        Rc::new(bytecode.child_protos),
    );
    // Carry the entry function's builder-idiom merge metadata so the alloc
    // dispatch mint-or-reuses merged slots (docs/impl/region/merging.md § Merging).
    code.merged_slots = bytecode.merged_slots;
    let empty_env = Rc::new(vec![]);

    // Drive the evaluated code, including any nested fiber/resume SIG_SWITCH
    // trampoline, to completion — see VM::run_thunk_to_completion.
    let bits = vm.run_thunk_to_completion(&code, &empty_env);

    match bits {
        SIG_OK => {
            let (_, value) = vm.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
            Ok(value)
        }
        SIG_ERROR => {
            let (_, err_value) = vm.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            Err(LError::generic(vm.format_error_with_location(err_value)))
        }
        _ => {
            // The refused suspend-class park is abandoned with its host.
            vm.abandon_hosted_park(bits);
            Err(LError::generic(format!(
                "eval: unexpected signal: {}",
                bits
            )))
        }
    }
}

/// Extract symbol-keyed entries from a struct value into a name→value map
/// suitable for `bind_compile_time_env`.
fn extract_env_bindings(
    env_value: Value,
    symbols: &SymbolTable,
) -> LResult<HashMap<String, Value>> {
    // Try immutable struct first, then mutable struct
    if let Some(entries) = env_value.as_struct() {
        let mut map = HashMap::new();
        for (key, value) in entries {
            if let TableKey::Symbol(sym_id) = key {
                if let Some(name) = symbols.name(*sym_id) {
                    map.insert(name.to_string(), *value);
                }
            }
            // Non-symbol keys (keywords, ints, etc.) are silently skipped
        }
        return Ok(map);
    }
    if let Some(cell) = env_value.as_struct_mut() {
        let borrowed = cell.borrow();
        let mut map = HashMap::new();
        for (key, value) in borrowed.iter() {
            if let TableKey::Symbol(sym_id) = key {
                if let Some(name) = symbols.name(*sym_id) {
                    map.insert(name.to_string(), *value);
                }
            }
        }
        return Ok(map);
    }
    Err(LError::generic(format!(
        "eval: env argument must be a struct or nil, got {}",
        env_value.type_name()
    )))
}
