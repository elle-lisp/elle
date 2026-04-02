//! Runtime eval instruction handler.
//!
//! Compiles and executes a datum (quoted value) at runtime.
//! The expression is compiled in an empty environment (just primitives
//! and prelude). The optional env argument on the stack is consumed
//! but ignored.

use crate::error::{LError, LResult};
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::lir::{Emitter, Lowerer};
use crate::primitives::cached_primitive_meta;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax};
use crate::value::{error_val, Value, SIG_ERROR, SIG_OK};
use std::rc::Rc;

use super::core::VM;

/// Handle the Eval instruction from the dispatch loop.
///
/// Accesses the symbol table via the thread-local context (same pattern
/// as FFI primitives). The symbol table must be set via
/// `set_symbol_table()` before execution.
pub(crate) fn handle_eval_instruction(vm: &mut VM) {
    let expr_value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on eval (expr)");
    // Pop the env argument from the stack (bytecode always pushes two
    // operands for Eval). The env is ignored — eval compiles in an
    // empty environment.
    let _env_value = vm
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

    match eval_inner(vm, expr_value, symbols) {
        Ok(result) => {
            vm.fiber.stack.push(result);
        }
        Err(msg) => {
            vm.fiber.signal = Some((SIG_ERROR, error_val("eval-error", msg)));
            vm.fiber.stack.push(Value::NIL);
        }
    }
}

fn eval_inner(vm: &mut VM, expr_value: Value, symbols: &mut SymbolTable) -> LResult<Value> {
    // Convert value to Syntax
    let span = Span::synthetic();
    let syntax = Syntax::from_value(&expr_value, symbols, span)?;

    // Get-or-create Expander (cached on VM)
    let mut expander = vm.eval_expander.take().unwrap_or_default();

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

    // Analyze
    let meta = cached_primitive_meta(symbols);
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer
        .analyze(&expanded)
        .map_err(|e| LError::generic(format!("eval: analysis failed: {}", e)))?;
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);

    // Mark tail calls
    mark_tail_calls(&mut analysis.hir);

    // Lower
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let mut lowerer = Lowerer::new(&arena)
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbols.all_names());
    let lir_module = lowerer
        .lower(&analysis.hir)
        .map_err(|e| LError::generic(format!("eval: lowering failed: {}", e)))?;

    // Emit
    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    // Execute
    let bc_rc = Rc::new(bytecode.instructions);
    let consts_rc = Rc::new(bytecode.constants);
    let location_map_rc = Rc::new(bytecode.location_map);
    let empty_env = Rc::new(vec![]);

    let result = vm.execute_bytecode_saving_stack(&bc_rc, &consts_rc, &empty_env, &location_map_rc);

    let mut bits = result.bits;

    // Handle SIG_SWITCH (fiber/resume trampoline) iteratively.
    while bits == crate::value::SIG_SWITCH {
        bits = vm.handle_sig_switch();
    }

    match bits {
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
            bits
        ))),
    }
}
