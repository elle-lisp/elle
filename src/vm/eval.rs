//! Runtime eval instruction handler.
//!
//! Compiles and executes a datum (quoted value) at runtime.
//! Supports an optional environment table for injecting bindings.

use crate::error::{LError, LResult};
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::Analyzer;
use crate::lir::{Emitter, Lowerer};
use crate::primitives::cached_primitive_meta;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};
use crate::value::heap::TableKey;
use crate::value::{error_val, Value, SIG_ERROR, SIG_OK};
use std::rc::Rc;

use super::core::VM;

/// Handle the Eval instruction from the dispatch loop.
///
/// Accesses the symbol table via the thread-local context (same pattern
/// as FFI primitives). The symbol table must be set via
/// `set_symbol_table()` before execution.
pub fn handle_eval_instruction(vm: &mut VM) {
    let expr_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on eval (expr)");
    let env_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on eval (env)");

    // Get symbol table from thread-local context
    let symbols_ptr = unsafe { crate::context::get_symbol_table() };
    let Some(symbols_ptr) = symbols_ptr else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "eval-error",
                "eval: symbol table not available (not set in context)",
            ),
        ));
        vm.fiber.stack.push(Value::NIL);
        return;
    };
    let symbols = unsafe { &mut *symbols_ptr };

    match eval_inner(vm, expr_value, env_value, symbols) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("eval-error", msg)));
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
    let expr_syntax = Syntax::from_value(&expr_value, symbols, span.clone())?;

    // If env is not nil, wrap in a let expression
    let syntax = if !env_value.is_nil() {
        wrap_with_env(expr_syntax, &env_value, symbols)?
    } else {
        expr_syntax
    };

    // Get-or-create Expander (cached on VM)
    let mut expander = vm.eval_expander.take().unwrap_or_default();

    // Save the caller's stack before macro expansion. load_prelude and
    // expand both execute VM bytecode (via eval_syntax → vm.execute)
    // which shares the same fiber stack. Without saving, macro expansion
    // overwrites the caller's local variable slots — corrupting cells
    // that hold destructured bindings.
    let saved_stack = std::mem::take(&mut vm.fiber.stack);
    let saved_allocator = crate::value::fiber_heap::save_active_allocator();

    // Load prelude if this is a fresh expander
    if !expander.has_macros() {
        if let Err(e) = expander.load_prelude(symbols, vm) {
            vm.fiber.stack = saved_stack;
            crate::value::fiber_heap::restore_active_allocator(saved_allocator);
            vm.eval_expander = Some(expander);
            return Err(LError::generic(format!("eval: prelude load failed: {}", e)));
        }
    }

    // Expand
    let expanded = match expander.expand(syntax, symbols, vm) {
        Ok(e) => e,
        Err(e) => {
            vm.fiber.stack = saved_stack;
            crate::value::fiber_heap::restore_active_allocator(saved_allocator);
            vm.eval_expander = Some(expander);
            return Err(LError::generic(format!("eval: expansion failed: {}", e)));
        }
    };

    // Restore the caller's stack after macro expansion
    vm.fiber.stack = saved_stack;
    crate::value::fiber_heap::restore_active_allocator(saved_allocator);

    // Put Expander back
    vm.eval_expander = Some(expander);

    // Analyze
    let meta = cached_primitive_meta(symbols);
    let mut analyzer = Analyzer::new_with_primitives(symbols, meta.effects, meta.arities);
    let mut analysis = analyzer
        .analyze(&expanded)
        .map_err(|e| LError::generic(format!("eval: analysis failed: {}", e)))?;

    // Mark tail calls
    mark_tail_calls(&mut analysis.hir);

    // Lower
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let mut lowerer = Lowerer::new()
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims);
    let lir_func = lowerer
        .lower(&analysis.hir)
        .map_err(|e| LError::generic(format!("eval: lowering failed: {}", e)))?;

    // Emit
    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let (bytecode, _yield_points, _call_sites) = emitter.emit(&lir_func);

    // Execute
    let bc_rc = Rc::new(bytecode.instructions);
    let consts_rc = Rc::new(bytecode.constants);
    let location_map_rc = Rc::new(bytecode.location_map);
    let empty_env = Rc::new(vec![]);

    let result = vm.execute_bytecode_saving_stack(&bc_rc, &consts_rc, &empty_env, &location_map_rc);

    match result.bits {
        SIG_OK => {
            let (_, value) = vm.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
            Ok(value)
        }
        SIG_ERROR => {
            let (_, err_value) = vm.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            Err(LError::generic(vm.format_error_with_location(err_value)))
        }
        _ => Err(LError::generic(format!(
            "eval: unexpected signal: {}",
            result.bits
        ))),
    }
}

/// Wrap an expression syntax in a let form with bindings from an env table/struct.
///
/// Given `expr` and `{:x 10 :y 20}`, produces:
/// `(let ((x 10) (y 20)) expr)`
fn wrap_with_env(expr_syntax: Syntax, env_value: &Value, symbols: &SymbolTable) -> LResult<Syntax> {
    let span = Span::synthetic();

    // Try mutable table first, then immutable struct
    let entries: Vec<(String, Value)> = if let Some(table_ref) = env_value.as_table() {
        let table = table_ref.borrow();
        table
            .iter()
            .map(|(k, v)| {
                let name = match k {
                    TableKey::Keyword(name) => Ok(name.clone()),
                    _ => Err(LError::generic("eval: env keys must be keywords")),
                };
                name.map(|n| (n, *v))
            })
            .collect::<LResult<Vec<_>>>()?
    } else if let Some(struct_ref) = env_value.as_struct() {
        struct_ref
            .iter()
            .map(|(k, v)| {
                let name = match k {
                    TableKey::Keyword(name) => Ok(name.clone()),
                    _ => Err(LError::generic("eval: env keys must be keywords")),
                };
                name.map(|n| (n, *v))
            })
            .collect::<LResult<Vec<_>>>()?
    } else {
        return Err(LError::generic("eval: env must be a table or struct"));
    };

    let mut bindings = Vec::new();
    for (name, val) in entries {
        // Skip bindings whose values can't be converted to Syntax
        // (closures, fibers, etc.). Only include literal-convertible values.
        let val_syntax = match Syntax::from_value(&val, symbols, span.clone()) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let name_syntax = Syntax::new(SyntaxKind::Symbol(name), span.clone());
        let binding_pair = Syntax::new(
            SyntaxKind::List(vec![name_syntax, val_syntax]),
            span.clone(),
        );
        bindings.push(binding_pair);
    }

    let let_sym = Syntax::new(SyntaxKind::Symbol("let".to_string()), span.clone());
    let bindings_list = Syntax::new(SyntaxKind::List(bindings), span.clone());

    Ok(Syntax::new(
        SyntaxKind::List(vec![let_sym, bindings_list, expr_syntax]),
        span,
    ))
}
