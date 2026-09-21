// audited: 2026-09-20
//! Running one prompt form: compile it, execute it, print what it answered,
//! and register what it bound.
//!
//! docs/pipeline.md
//! docs/impl/region/rules.md
//!
//! A `def` at the prompt has to outlive the line that produced it, so its
//! value goes into the compilation cache as a REPL binding — the mechanism the
//! stdlib exports use — and the next line resolves the name from there. That
//! binding mints the reference that keeps it, because the value it is made of
//! belongs to the form's own answer, which the print gives back.

use crate::pipeline::{compile_file_repl, CompileCtx};
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::value::arena::{release_program_value, RootRef};
use crate::value::types::Arity;
use crate::value::Value;
use crate::vm::VM;

use super::defer::{try_defer, try_resolve_deferred, DeferredForm};
use super::read::{try_read, FormInfo, ReadResult};

/// Try to parse and evaluate accumulated input.
/// Clears `accumulated` on success or hard error. Leaves it intact on
/// incomplete input. Returns true if an error occurred.
pub(super) fn try_eval_accumulated(
    accumulated: &mut String,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    deferred: &mut Vec<DeferredForm>,
) -> bool {
    let mut had_errors = false;

    match try_read(accumulated) {
        ReadResult::Complete(forms) => {
            accumulated.clear();
            for form in &forms {
                match eval_form(form, vm, symbols, cctx) {
                    Ok(value) => {
                        if !value.is_nil() {
                            // Debug render through the instance memo, so symbol
                            // and keyword spellings resolve.
                            println!("⟹ {}", value.debug_with(Some(symbols)));
                        }
                        // The print is the last read of the form's value, so
                        // the owning reference it arrived with goes back here
                        // (docs/impl/region/rules.md). Whatever the form bound
                        // holds a reference of its own.
                        release_program_value(unsafe { &mut *vm.heap_ptr }, value);
                    }
                    Err(e) => {
                        if let Some(d) = try_defer(form, &e) {
                            eprintln!("{}: deferred ({} undefined)", d.name, d.missing.join(", "));
                            deferred.push(d);
                        } else {
                            eprintln!("✗ {}", e);
                            had_errors = true;
                        }
                    }
                }
                try_resolve_deferred(deferred, vm, symbols, cctx);
            }
        }
        ReadResult::Incomplete => {}
        ReadResult::Error(e) => {
            eprintln!("✗ {}", e);
            accumulated.clear();
            had_errors = true;
        }
    }

    had_errors
}

/// Compile and execute a single form. If it introduces bindings
/// (def/var/defn, including destructuring), register each in the
/// compilation cache so subsequent forms see them.
///
/// Answers the form's value holding the one owning reference the return
/// convention minted, whichever branch ran — so the caller releases it once
/// and needs to know nothing about what the form bound. Every binding
/// registered here therefore mints a reference of its own: the simple `def`
/// binds the very value being answered, and each leaf of a destructuring `def`
/// belongs to the tuple that is.
fn eval_form(
    form: &FormInfo,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<Value, String> {
    if form.bindings.len() <= 1 {
        // No bindings or simple def: compile the form as-is.
        // For simple def, the letrec body is the bound name, so the
        // return value IS the bound value.
        let (result, expander) = compile_file_repl(&form.source, symbols, cctx, "<repl>")?;
        cctx.register_repl_macros(expander.macros());
        let value = vm.execute_scheduled(&result.bytecode, cctx)?;

        if let Some(binding) = form.bindings.first() {
            let sym_id = symbols.intern(&binding.name);
            let (signal, arity) = extract_signal_arity(&value);
            cctx.register_repl_binding(
                unsafe { &mut *vm.heap_ptr },
                sym_id,
                value,
                RootRef::Mint,
                signal,
                arity,
            );
        }

        Ok(value)
    } else {
        // Destructuring def: compile the def followed by a tuple of
        // all leaf names. compile_file wraps both in a letrec, so the
        // tuple expression can reference the destructured bindings.
        // The return value is the tuple; we unpack it to register each.
        let names: Vec<&str> = form.bindings.iter().map(|b| b.name.as_str()).collect();
        let trailer = format!("[{}]", names.join(" "));
        let combined = format!("{} {}", form.source, trailer);

        let (result, expander) = compile_file_repl(&combined, symbols, cctx, "<repl>")?;
        cctx.register_repl_macros(expander.macros());
        let tuple_val = vm.execute_scheduled(&result.bytecode, cctx)?;

        // Register each leaf binding from the tuple.
        if let Some(items) = tuple_val.as_array() {
            for (binding, val) in form.bindings.iter().zip(items.iter()) {
                let sym_id = symbols.intern(&binding.name);
                let (signal, arity) = extract_signal_arity(val);
                cctx.register_repl_binding(
                    unsafe { &mut *vm.heap_ptr },
                    sym_id,
                    *val,
                    RootRef::Mint,
                    signal,
                    arity,
                );
            }
        }

        Ok(tuple_val)
    }
}

/// Extract signal and arity from a runtime value.
pub(super) fn extract_signal_arity(value: &Value) -> (Signal, Option<Arity>) {
    match value.as_closure() {
        Some(closure) => (closure.effective_signal(), Some(closure.template.arity())),
        None => (Signal::silent(), None),
    }
}
